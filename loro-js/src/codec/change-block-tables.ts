import { ByteReader, ByteWriter, concatBytes } from "./bytes";
import { containerTypeFromRawByte, containerTypeToRawByte } from "./container-id";
import { LoroDecodeError, LoroEncodeError, decodeAssert } from "./errors";
import { U64_MAX, readUlebNumber, writeUleb128 } from "./leb128";
import { PostcardReader, PostcardWriter } from "./postcard";
import {
  decodeColumnarVecMaybeWrapped,
  decodeDeltaRleI32,
  decodeDeltaRleIsize,
  decodeDeltaRleU32,
  decodeDeltaRleUsize,
  decodeRleU8,
  decodeRleU32,
  encodeAnyRleU32,
  encodeAnyRleUsize,
  encodeBoolRle,
  encodeColumnarVecWrapped,
  encodeDeltaOfDeltaI64,
  encodeDeltaRleI32,
  encodeDeltaRleIsize,
  encodeDeltaRleU32,
  encodeDeltaRleUsize,
  encodeRleU8,
  encodeRleU32,
  takeAnyRleU32,
  takeAnyRleUsize,
  takeBoolRle,
  takeDeltaOfDeltaI64,
} from "./serde-columnar";
import type { ContainerId, Id } from "./types";

const textDecoder = new TextDecoder("utf-8", { fatal: true });
const textEncoder = new TextEncoder();
const I32_MIN = -0x8000_0000;
const I32_MAX = 0x7fff_ffff;
const EMPTY_SINGLE_CHANGE_HEADER_SUFFIX = Uint8Array.of(1, 0, 0, 0, 0, 0);

export interface ChangesHeader {
  readonly peer: bigint;
  readonly peers: readonly bigint[];
  readonly counters: readonly number[];
  readonly lengths: readonly number[];
  readonly lamports: readonly number[];
  readonly dependencies: readonly (readonly Id[])[];
}

export interface ChangesMetadata {
  readonly timestamps: readonly bigint[];
  readonly commitMessages: readonly (string | undefined)[];
}

export interface EncodedOperationRow {
  readonly containerIndex: number;
  readonly property: number;
  readonly valueType: number;
  readonly length: number;
}

export interface EncodedDeleteStartIdRow {
  readonly peerIndex: bigint;
  readonly counter: number;
  readonly length: bigint;
}

export interface DecodeChangesHeaderOptions {
  readonly changeCount: number;
  readonly counterStart: number;
  readonly counterLength: number;
  readonly lamportStart: number;
  readonly lamportLength: number;
}

export function decodeChangesHeader(
  bytes: Uint8Array,
  options: DecodeChangesHeaderOptions,
): ChangesHeader {
  const changeCount = options.changeCount;
  decodeAssert(
    Number.isSafeInteger(changeCount) && changeCount > 0 && changeCount <= 10_000_000,
    "invalid change count",
  );
  const firstCounter = options.counterStart | 0;
  decodeAssert(options.counterLength <= I32_MAX, "change counter length is out of range");
  const reader = new ByteReader(bytes);
  const peerCount = readUlebNumber(reader, 10_000_000);
  decodeAssert(peerCount > 0, "change header has an empty peer table");
  const peers: bigint[] = [];
  for (let index = 0; index < peerCount; index += 1) {
    peers.push(reader.readU64LE());
  }

  const lengths: number[] = [];
  let knownLength = 0;
  for (let index = 0; index < changeCount - 1; index += 1) {
    const length = readUlebNumber(reader, I32_MAX);
    knownLength = checkedI32(knownLength + length, "change counter length");
    lengths.push(length);
  }
  const finalLength = options.counterLength - knownLength;
  decodeAssert(finalLength >= 0, "change lengths exceed the block counter range");
  lengths.push(finalLength);

  let remaining = bytes.subarray(reader.position);
  let dependencyOnSelf: boolean[];
  [dependencyOnSelf, remaining] = takeBoolRle(remaining, changeCount);
  let dependencyLengthsBigInt: bigint[];
  [dependencyLengthsBigInt, remaining] = takeAnyRleUsize(remaining, changeCount);
  const dependencyLengths = dependencyLengthsBigInt.map((value) =>
    bigintToNumber(value, 10_000_000, "dependency length"),
  );
  const otherDependencyCount = dependencyLengths.reduce((sum, value) => sum + value, 0);
  decodeAssert(otherDependencyCount <= 10_000_000, "too many change dependencies");
  let dependencyPeerIndices: bigint[];
  [dependencyPeerIndices, remaining] = takeAnyRleUsize(remaining, otherDependencyCount);
  let dependencyCounters: bigint[];
  [dependencyCounters, remaining] = takeDeltaOfDeltaI64(remaining, otherDependencyCount);

  const dependencies: Id[][] = [];
  let dependencyIndex = 0;
  let counter = firstCounter;
  for (let index = 0; index < changeCount; index += 1) {
    const ids: Id[] = [];
    if (dependencyOnSelf[index]) {
      decodeAssert(counter > I32_MIN, "self dependency counter underflow");
      ids.push({ peer: peers[0]!, counter: counter - 1 });
    }
    for (let dep = 0; dep < dependencyLengths[index]!; dep += 1) {
      const peerIndex = bigintToNumber(
        dependencyPeerIndices[dependencyIndex]!,
        peers.length - 1,
        "dependency peer index",
      );
      const dependencyCounter = bigintToNumber(
        dependencyCounters[dependencyIndex]!,
        I32_MAX,
        "dependency counter",
      );
      ids.push({ peer: peers[peerIndex]!, counter: dependencyCounter });
      dependencyIndex += 1;
    }
    dependencies.push(ids);
    counter = checkedI32(counter + lengths[index]!, "change counter");
  }
  decodeAssert(
    dependencyIndex === dependencyPeerIndices.length &&
      dependencyIndex === dependencyCounters.length,
    "trailing change dependencies",
  );

  const counters: number[] = [];
  counter = firstCounter;
  for (const length of lengths) {
    counters.push(counter);
    counter = checkedI32(counter + length, "change counter");
  }
  counters.push(counter);

  let encodedLamports: bigint[];
  [encodedLamports, remaining] = takeDeltaOfDeltaI64(remaining, changeCount - 1);
  decodeAssert(remaining.length === 0, "trailing change header bytes");
  const lamports = encodedLamports.map((value) =>
    bigintToNumber(value, 0xffff_ffff, "lamport"),
  );
  const blockLamportEnd = options.lamportStart + options.lamportLength;
  decodeAssert(blockLamportEnd <= 0xffff_ffff, "lamport range overflow");
  const lastLamport = blockLamportEnd - finalLength;
  decodeAssert(
    Number.isSafeInteger(lastLamport) && lastLamport >= 0 && lastLamport <= 0xffff_ffff,
    "invalid final lamport",
  );
  lamports.push(lastLamport);
  return {
    peer: peers[0]!,
    peers,
    counters,
    lengths,
    lamports,
    dependencies,
  };
}

export function encodeChangesHeader(header: ChangesHeader): Uint8Array {
  const changeCount = header.lengths.length;
  if (
    changeCount === 0 ||
    header.peers.length === 0 ||
    header.peers[0] !== header.peer ||
    header.dependencies.length !== changeCount ||
    header.lamports.length !== changeCount
  ) {
    throw new LoroEncodeError("inconsistent change header arrays");
  }
  if (changeCount === 1 && header.peers.length === 1) {
    const dependencies = header.dependencies[0]!;
    const selfDependent =
      dependencies.length === 1 &&
      dependencies[0]!.peer === header.peer &&
      dependencies[0]!.counter === header.counters[0]! - 1;
    if (dependencies.length === 0 || selfDependent) {
      if (header.peer < 0n || header.peer > U64_MAX) {
        throw new LoroEncodeError(`u64 is out of range: ${header.peer}`);
      }
      const output = new Uint8Array(selfDependent ? 17 : 16);
      // Canonical one-change headers contain one peer and no explicit
      // length/lamport columns. Only the self-dependency columns differ.
      output[0] = 1;
      new DataView(output.buffer).setBigUint64(1, header.peer, true);
      let offset = 9;
      if (selfDependent) {
        output[offset] = 0;
        output[offset + 1] = 1;
        offset += 2;
      } else {
        output[offset] = 1;
        offset += 1;
      }
      output.set(EMPTY_SINGLE_CHANGE_HEADER_SUFFIX, offset);
      return output;
    }
  }
  const writer = new ByteWriter();
  writeUleb128(writer, header.peers.length);
  for (const peer of header.peers) {
    writer.writeU64LE(peer);
  }
  for (let index = 0; index < changeCount - 1; index += 1) {
    writeUleb128(writer, assertNonnegativeI32(header.lengths[index]!, "change length"));
  }
  const peerIndices = new Map(header.peers.map((peer, index) => [peer, index]));
  const selfDependencies: boolean[] = [];
  const dependencyLengths: bigint[] = [];
  const dependencyPeerIndices: bigint[] = [];
  const dependencyCounters: bigint[] = [];
  for (let index = 0; index < changeCount; index += 1) {
    const expectedSelfCounter = header.counters[index]! - 1;
    let hasSelf = false;
    let otherCount = 0;
    for (const dependency of header.dependencies[index]!) {
      if (dependency.peer === header.peer) {
        if (hasSelf || dependency.counter !== expectedSelfCounter) {
          throw new LoroEncodeError("invalid same-peer change dependency");
        }
        hasSelf = true;
      } else {
        const peerIndex = peerIndices.get(dependency.peer);
        if (peerIndex === undefined) {
          throw new LoroEncodeError(`dependency peer is absent from the peer table`);
        }
        dependencyPeerIndices.push(BigInt(peerIndex));
        dependencyCounters.push(
          BigInt(assertNonnegativeI32(dependency.counter, "dependency counter")),
        );
        otherCount += 1;
      }
    }
    selfDependencies.push(hasSelf);
    dependencyLengths.push(BigInt(otherCount));
  }
  writer.writeBytes(encodeBoolRle(selfDependencies));
  writer.writeBytes(encodeAnyRleUsize(dependencyLengths));
  writer.writeBytes(encodeAnyRleUsize(dependencyPeerIndices));
  writer.writeBytes(encodeDeltaOfDeltaI64(dependencyCounters));
  writer.writeBytes(
    encodeDeltaOfDeltaI64(header.lamports.slice(0, -1).map((value) => BigInt(value))),
  );
  return writer.toUint8Array();
}

export function decodeChangesMetadata(
  bytes: Uint8Array,
  changeCount: number,
): ChangesMetadata {
  let remaining = bytes;
  let timestamps: bigint[];
  [timestamps, remaining] = takeDeltaOfDeltaI64(remaining, changeCount);
  let messageLengths: number[];
  [messageLengths, remaining] = takeAnyRleU32(remaining, changeCount);
  const totalLength = messageLengths.reduce((sum, value) => sum + value, 0);
  decodeAssert(totalLength === remaining.length, "commit message byte length mismatch");
  const commitMessages: (string | undefined)[] = [];
  let offset = 0;
  for (const length of messageLengths) {
    if (length === 0) {
      commitMessages.push(undefined);
      continue;
    }
    const messageBytes = remaining.subarray(offset, offset + length);
    try {
      commitMessages.push(textDecoder.decode(messageBytes));
    } catch {
      throw new LoroDecodeError("invalid UTF-8 commit message", offset);
    }
    offset += length;
  }
  return { timestamps, commitMessages };
}

export function encodeChangesMetadata(metadata: ChangesMetadata): Uint8Array {
  if (metadata.timestamps.length !== metadata.commitMessages.length) {
    throw new LoroEncodeError("inconsistent change metadata arrays");
  }
  if (
    metadata.timestamps.length === 1 &&
    metadata.timestamps[0] === 0n &&
    metadata.commitMessages[0] === undefined
  ) {
    return Uint8Array.of(1, 0, 0, 1, 0);
  }
  const lengths: number[] = [];
  const messages: Uint8Array[] = [];
  for (const message of metadata.commitMessages) {
    if (message === undefined) {
      lengths.push(0);
    } else {
      const bytes = textEncoder.encode(message);
      lengths.push(bytes.length);
      messages.push(bytes);
    }
  }
  return concatBytes(
    encodeDeltaOfDeltaI64(metadata.timestamps),
    encodeAnyRleU32(lengths),
    ...messages,
  );
}

export function decodeChangeKeys(bytes: Uint8Array): string[] {
  const reader = new ByteReader(bytes);
  const keys: string[] = [];
  while (reader.remaining > 0) {
    const offset = reader.position;
    const value = reader.readBytes(readUlebNumber(reader, 0x7fff_ffff));
    try {
      keys.push(textDecoder.decode(value));
    } catch {
      throw new LoroDecodeError("invalid UTF-8 change key", offset);
    }
  }
  return keys;
}

export function encodeChangeKeys(keys: readonly string[]): Uint8Array {
  if (keys.length === 1 && keys[0]!.length <= 0x7f) {
    const key = keys[0]!;
    let ascii = true;
    for (let index = 0; ascii && index < key.length; index += 1) {
      ascii = key.charCodeAt(index) < 0x80;
    }
    if (ascii) {
      const output = new Uint8Array(key.length + 1);
      output[0] = key.length;
      for (let index = 0; index < key.length; index += 1) {
        output[index + 1] = key.charCodeAt(index);
      }
      return output;
    }
  }
  const writer = new ByteWriter();
  for (const key of keys) {
    const bytes = textEncoder.encode(key);
    writeUleb128(writer, bytes.length);
    writer.writeBytes(bytes);
  }
  return writer.toUint8Array();
}

export function decodeContainerArena(
  bytes: Uint8Array,
  peers: readonly bigint[],
  keys: readonly string[],
): ContainerId[] {
  const reader = new PostcardReader(bytes);
  const count = reader.readUsize();
  decodeAssert(count <= 10_000_000, "container arena is too large");
  const containers: ContainerId[] = [];
  for (let index = 0; index < count; index += 1) {
    decodeAssert(reader.readUsize() === 4, "invalid encoded container field count");
    const isRoot = reader.readBool();
    const containerType = containerTypeFromRawByte(reader.readU8());
    const peerIndex = reader.readUsize();
    const keyIndexOrCounter = reader.readI32();
    if (isRoot) {
      decodeAssert(
        keyIndexOrCounter >= 0 && keyIndexOrCounter < keys.length,
        "invalid root container key index",
      );
      containers.push({
        kind: "root",
        name: keys[keyIndexOrCounter]!,
        containerType,
      });
    } else {
      decodeAssert(peerIndex < peers.length, "invalid normal container peer index");
      containers.push({
        kind: "normal",
        peer: peers[peerIndex]!,
        counter: keyIndexOrCounter,
        containerType,
      });
    }
  }
  reader.assertEnd();
  return containers;
}

export function encodeContainerArena(
  containers: readonly ContainerId[],
  peers: readonly bigint[],
  keys: readonly string[],
): Uint8Array {
  if (
    containers.length === 1 &&
    containers[0]!.kind === "root" &&
    keys.length === 1 &&
    keys[0] === containers[0]!.name
  ) {
    return Uint8Array.of(
      1,
      4,
      1,
      containerTypeToRawByte(containers[0]!.containerType),
      0,
      0,
    );
  }
  const writer = new PostcardWriter();
  writer.writeUsize(containers.length);
  const peerIndices = new Map(peers.map((peer, index) => [peer, index]));
  const keyIndices = new Map(keys.map((key, index) => [key, index]));
  for (const container of containers) {
    writer.writeUsize(4);
    writer.writeBool(container.kind === "root");
    writer.writeU8(containerTypeToRawByte(container.containerType));
    if (container.kind === "root") {
      const keyIndex = keyIndices.get(container.name);
      if (keyIndex === undefined) {
        throw new LoroEncodeError(`root container name is absent from the key table`);
      }
      writer.writeUsize(0);
      writer.writeI32(keyIndex);
    } else {
      const peerIndex = peerIndices.get(container.peer);
      if (peerIndex === undefined) {
        throw new LoroEncodeError(`container peer is absent from the peer table`);
      }
      writer.writeUsize(peerIndex);
      writer.writeI32(container.counter);
    }
  }
  return writer.toUint8Array();
}

export function decodeEncodedOperations(bytes: Uint8Array): EncodedOperationRow[] {
  if (bytes.length === 0) {
    return [];
  }
  const columns = decodeColumnarVecMaybeWrapped(bytes);
  decodeAssert(columns.length === 4, "encoded operations must have four columns");
  const containerIndices = decodeDeltaRleU32(columns[0]!);
  const properties = decodeDeltaRleI32(columns[1]!);
  const valueTypes = decodeRleU8(columns[2]!);
  const lengths = decodeRleU32(columns[3]!);
  decodeAssert(
    properties.length === containerIndices.length &&
      valueTypes.length === containerIndices.length &&
      lengths.length === containerIndices.length,
    "encoded operation column length mismatch",
  );
  return containerIndices.map((containerIndex, index) => ({
    containerIndex,
    property: properties[index]!,
    valueType: valueTypes[index]!,
    length: lengths[index]!,
  }));
}

export function encodeEncodedOperations(
  rows: readonly EncodedOperationRow[],
): Uint8Array {
  if (rows.length === 1) {
    const row = rows[0]!;
    const containerIndex = assertU32(row.containerIndex, "u32");
    const property = assertI32(row.property, "i32");
    const valueType = assertU8(row.valueType, "u8");
    const operationLength = assertU32(row.length, "u32");
    const encodedContainer = containerIndex * 2;
    const encodedProperty = property >= 0 ? property * 2 : -property * 2 - 1;
    const containerLength = 1 + ulebNumberLength(encodedContainer);
    const propertyLength = 1 + ulebNumberLength(encodedProperty);
    const operationLengthLength = 1 + ulebNumberLength(operationLength);
    const output = new Uint8Array(
      2 + 1 + containerLength + 1 + propertyLength + 3 + 1 + operationLengthLength,
    );
    // Wrapped column vector followed by four one-value columns.
    let offset = 0;
    output[offset++] = 1;
    output[offset++] = 4;
    output[offset++] = containerLength;
    output[offset++] = 1;
    offset = writeUlebNumber(output, offset, encodedContainer);
    output[offset++] = propertyLength;
    output[offset++] = 1;
    offset = writeUlebNumber(output, offset, encodedProperty);
    output[offset++] = 2;
    output[offset++] = 1;
    output[offset++] = valueType;
    output[offset++] = operationLengthLength;
    output[offset++] = 1;
    writeUlebNumber(output, offset, operationLength);
    return output;
  }
  return encodeColumnarVecWrapped([
    encodeDeltaRleU32(rows.map((row) => row.containerIndex)),
    encodeDeltaRleI32(rows.map((row) => row.property)),
    encodeRleU8(rows.map((row) => row.valueType)),
    encodeRleU32(rows.map((row) => row.length)),
  ]);
}

export function decodeDeleteStartIds(bytes: Uint8Array): EncodedDeleteStartIdRow[] {
  if (bytes.length === 0) {
    return [];
  }
  const columns = decodeColumnarVecMaybeWrapped(bytes);
  decodeAssert(columns.length === 3, "delete start IDs must have three columns");
  const peerIndices = decodeDeltaRleUsize(columns[0]!);
  const counters = decodeDeltaRleI32(columns[1]!);
  const lengths = decodeDeltaRleIsize(columns[2]!);
  decodeAssert(
    counters.length === peerIndices.length && lengths.length === peerIndices.length,
    "delete start ID column length mismatch",
  );
  return peerIndices.map((peerIndex, index) => ({
    peerIndex,
    counter: counters[index]!,
    length: lengths[index]!,
  }));
}

export function encodeDeleteStartIds(
  rows: readonly EncodedDeleteStartIdRow[],
): Uint8Array {
  if (rows.length === 0) {
    return new Uint8Array();
  }
  return encodeColumnarVecWrapped([
    encodeDeltaRleUsize(rows.map((row) => row.peerIndex)),
    encodeDeltaRleI32(rows.map((row) => row.counter)),
    encodeDeltaRleIsize(rows.map((row) => row.length)),
  ]);
}

function checkedI32(value: number, label: string): number {
  decodeAssert(
    Number.isSafeInteger(value) && value >= I32_MIN && value <= I32_MAX,
    `${label} overflow`,
  );
  return value;
}

function bigintToNumber(value: bigint, max: number, label: string): number {
  decodeAssert(value >= 0n && value <= BigInt(max), `${label} is out of range`);
  return Number(value);
}

function assertNonnegativeI32(value: number, label: string): number {
  if (!Number.isSafeInteger(value) || value < 0 || value > I32_MAX) {
    throw new LoroEncodeError(`${label} is out of range: ${value}`);
  }
  return value;
}

function assertU8(value: number, label: string): number {
  if (!Number.isSafeInteger(value) || value < 0 || value > 0xff) {
    throw new LoroEncodeError(`${label} is out of range: ${value}`);
  }
  return value;
}

function assertU32(value: number, label: string): number {
  if (!Number.isSafeInteger(value) || value < 0 || value > 0xffff_ffff) {
    throw new LoroEncodeError(`${label} is out of range: ${value}`);
  }
  return value;
}

function assertI32(value: number, label: string): number {
  if (!Number.isSafeInteger(value) || value < I32_MIN || value > I32_MAX) {
    throw new LoroEncodeError(`${label} is out of range: ${value}`);
  }
  return value;
}

function writeUlebNumber(output: Uint8Array, offset: number, input: number): number {
  let value = input;
  do {
    let byte = value % 128;
    value = Math.floor(value / 128);
    if (value !== 0) {
      byte |= 0x80;
    }
    output[offset++] = byte;
  } while (value !== 0);
  return offset;
}

function ulebNumberLength(value: number): number {
  let length = 1;
  while (value >= 128) {
    value = Math.floor(value / 128);
    length += 1;
  }
  return length;
}
