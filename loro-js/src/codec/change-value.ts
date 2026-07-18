import { ByteReader, ByteWriter } from "./bytes";
import { LoroDecodeError, LoroEncodeError, decodeAssert } from "./errors";
import { readSleb128, readUleb128, writeSleb128, writeUleb128 } from "./leb128";

const textDecoder = new TextDecoder("utf-8", { fatal: true });
const textEncoder = new TextEncoder();
const MAX_VALUE_DEPTH = 1024;
const MAX_COLLECTION_LENGTH = 10_000_000;

export type ChangeLoroValue =
  | { readonly type: "null" }
  | { readonly type: "bool"; readonly value: boolean }
  | { readonly type: "i64"; readonly value: bigint }
  | { readonly type: "double"; readonly value: number }
  | { readonly type: "string"; readonly value: string }
  | { readonly type: "binary"; readonly value: Uint8Array }
  | { readonly type: "list"; readonly value: readonly ChangeLoroValue[] }
  | {
      readonly type: "map";
      readonly value: readonly (readonly [bigint, ChangeLoroValue])[];
    }
  | { readonly type: "container-type"; readonly value: number };

export type ChangeValue =
  | { readonly type: "null" }
  | { readonly type: "bool"; readonly value: boolean }
  | { readonly type: "i64"; readonly value: bigint }
  | { readonly type: "double"; readonly value: number }
  | { readonly type: "string"; readonly value: string }
  | { readonly type: "binary"; readonly value: Uint8Array }
  | { readonly type: "container-index"; readonly value: bigint }
  | { readonly type: "delete-once" }
  | { readonly type: "delete-sequence" }
  | { readonly type: "delta-int"; readonly value: number }
  | { readonly type: "loro-value"; readonly value: ChangeLoroValue }
  | {
      readonly type: "mark-start";
      readonly info: number;
      readonly length: bigint;
      readonly keyIndex: bigint;
      readonly value: ChangeLoroValue;
    }
  | {
      readonly type: "tree-move";
      readonly targetIndex: bigint;
      readonly parentIsNull: boolean;
      readonly position: bigint;
      readonly parentIndex: bigint | undefined;
    }
  | {
      readonly type: "list-move";
      readonly from: bigint;
      readonly fromPeerIndex: bigint;
      readonly lamport: bigint;
    }
  | {
      readonly type: "list-set";
      readonly peerIndex: bigint;
      readonly lamport: number;
      readonly value: ChangeLoroValue;
    }
  | {
      readonly type: "raw-tree-move";
      readonly subjectPeerIndex: bigint;
      readonly subjectCounter: number;
      readonly positionIndex: bigint;
      readonly parentIsNull: boolean;
      readonly parentPeerIndex: bigint;
      readonly parentCounter: number;
    }
  | { readonly type: "future"; readonly tag: number; readonly data: Uint8Array };

export interface EncodedChangeValueContent {
  readonly tag: number;
  readonly bytes: Uint8Array;
}

export function decodeChangeValue(bytes: Uint8Array): ChangeValue {
  const reader = new ByteReader(bytes);
  const tag = reader.readU8();
  const [value, remaining] = decodeChangeValueContent(tag, bytes.subarray(1));
  decodeAssert(remaining.length === 0, "trailing change value bytes");
  return value;
}

export function decodeChangeValueContent(
  tag: number,
  bytes: Uint8Array,
): [ChangeValue, Uint8Array] {
  const reader = new ByteReader(bytes);
  const kind = tag & 0x7f;
  let value: ChangeValue;
  switch (kind) {
    case 0:
      value = { type: "null" };
      break;
    case 1:
      value = { type: "bool", value: true };
      break;
    case 2:
      value = { type: "bool", value: false };
      break;
    case 3:
      value = { type: "i64", value: readSleb128(reader) };
      break;
    case 4:
      value = { type: "double", value: readF64BE(reader) };
      break;
    case 5:
      value = { type: "string", value: readUtf8(reader) };
      break;
    case 6:
      value = { type: "binary", value: readLengthPrefixedBytes(reader) };
      break;
    case 7:
      value = { type: "container-index", value: readUleb128(reader) };
      break;
    case 8:
      value = { type: "delete-once" };
      break;
    case 9:
      value = { type: "delete-sequence" };
      break;
    case 10: {
      const integer = readSleb128(reader);
      decodeAssert(
        integer >= -0x8000_0000n && integer <= 0x7fff_ffffn,
        "change delta integer is out of range",
      );
      value = { type: "delta-int", value: Number(integer) };
      break;
    }
    case 11:
      value = { type: "loro-value", value: readChangeLoroValue(reader) };
      break;
    case 12:
      value = {
        type: "mark-start",
        info: reader.readU8(),
        length: readUleb128(reader),
        keyIndex: readUleb128(reader),
        value: readChangeLoroValue(reader),
      };
      break;
    case 13: {
      const targetIndex = readUleb128(reader);
      const parentIsNull = reader.readU8() !== 0;
      const position = readUleb128(reader);
      value = {
        type: "tree-move",
        targetIndex,
        parentIsNull,
        position,
        parentIndex: parentIsNull ? undefined : readUleb128(reader),
      };
      break;
    }
    case 14:
      value = {
        type: "list-move",
        from: readUleb128(reader),
        fromPeerIndex: readUleb128(reader),
        lamport: readUleb128(reader),
      };
      break;
    case 15: {
      const peerIndex = readUleb128(reader);
      const lamport = readUleb128(reader);
      decodeAssert(lamport <= 0xffff_ffffn, "list-set lamport is out of range");
      value = {
        type: "list-set",
        peerIndex,
        lamport: Number(lamport),
        value: readChangeLoroValue(reader),
      };
      break;
    }
    case 16: {
      const subjectPeerIndex = readUleb128(reader);
      const subjectCounter = readNonnegativeI32(reader, "tree subject counter");
      const positionIndex = readUleb128(reader);
      const parentIsNull = reader.readU8() !== 0;
      const parentPeerIndex = parentIsNull ? 0n : readUleb128(reader);
      const parentCounter = parentIsNull
        ? 0
        : readNonnegativeI32(reader, "tree parent counter");
      value = {
        type: "raw-tree-move",
        subjectPeerIndex,
        subjectCounter,
        positionIndex,
        parentIsNull,
        parentPeerIndex,
        parentCounter,
      };
      break;
    }
    default:
      value = { type: "future", tag, data: readLengthPrefixedBytes(reader) };
  }
  return [value, bytes.subarray(reader.position)];
}

export function encodeChangeValue(value: ChangeValue): Uint8Array {
  const encoded = encodeChangeValueContent(value);
  const writer = new ByteWriter(encoded.bytes.length + 1);
  writer.writeU8(encoded.tag);
  writer.writeBytes(encoded.bytes);
  return writer.toUint8Array();
}

export function encodeChangeValueContent(value: ChangeValue): EncodedChangeValueContent {
  const writer = new ByteWriter();
  let tag: number;
  switch (value.type) {
    case "null":
      tag = 0;
      break;
    case "bool":
      tag = value.value ? 1 : 2;
      break;
    case "i64":
      tag = 3;
      writeSleb128(writer, value.value);
      break;
    case "double":
      tag = 4;
      writeF64BE(writer, value.value);
      break;
    case "string":
      tag = 5;
      writeUtf8(writer, value.value);
      break;
    case "binary":
      tag = 6;
      writeLengthPrefixedBytes(writer, value.value);
      break;
    case "container-index":
      tag = 7;
      writeUleb128(writer, value.value);
      break;
    case "delete-once":
      tag = 8;
      break;
    case "delete-sequence":
      tag = 9;
      break;
    case "delta-int":
      tag = 10;
      assertI32(value.value, "delta integer");
      writeSleb128(writer, value.value);
      break;
    case "loro-value":
      tag = 11;
      writeChangeLoroValue(writer, value.value);
      break;
    case "mark-start":
      tag = 12;
      writer.writeU8(value.info);
      writeUleb128(writer, value.length);
      writeUleb128(writer, value.keyIndex);
      writeChangeLoroValue(writer, value.value);
      break;
    case "tree-move":
      tag = 13;
      writeUleb128(writer, value.targetIndex);
      writer.writeU8(value.parentIsNull ? 1 : 0);
      writeUleb128(writer, value.position);
      if (!value.parentIsNull) {
        if (value.parentIndex === undefined) {
          throw new LoroEncodeError("tree move lacks its parent index");
        }
        writeUleb128(writer, value.parentIndex);
      }
      break;
    case "list-move":
      tag = 14;
      writeUleb128(writer, value.from);
      writeUleb128(writer, value.fromPeerIndex);
      writeUleb128(writer, value.lamport);
      break;
    case "list-set":
      tag = 15;
      writeUleb128(writer, value.peerIndex);
      writeUleb128(writer, value.lamport);
      writeChangeLoroValue(writer, value.value);
      break;
    case "raw-tree-move":
      tag = 16;
      writeUleb128(writer, value.subjectPeerIndex);
      writeUleb128(
        writer,
        assertNonnegativeI32(value.subjectCounter, "tree subject counter"),
      );
      writeUleb128(writer, value.positionIndex);
      writer.writeU8(value.parentIsNull ? 1 : 0);
      if (!value.parentIsNull) {
        writeUleb128(writer, value.parentPeerIndex);
        writeUleb128(
          writer,
          assertNonnegativeI32(value.parentCounter, "tree parent counter"),
        );
      }
      break;
    case "future":
      tag = value.tag;
      if (!Number.isSafeInteger(tag) || tag < 0 || tag > 0xff || (tag & 0x7f) <= 16) {
        throw new LoroEncodeError(`invalid future change value tag: ${tag}`);
      }
      writeLengthPrefixedBytes(writer, value.data);
  }
  return { tag, bytes: writer.toUint8Array() };
}

export function readChangeLoroValue(reader: ByteReader, depth = 0): ChangeLoroValue {
  decodeAssert(depth <= MAX_VALUE_DEPTH, "change LoroValue is too deep", reader.position);
  const kind = reader.readU8();
  switch (kind) {
    case 0:
      return { type: "null" };
    case 1:
      return { type: "bool", value: true };
    case 2:
      return { type: "bool", value: false };
    case 3:
      return { type: "i64", value: readSleb128(reader) };
    case 4:
      return { type: "double", value: readF64BE(reader) };
    case 5:
      return { type: "string", value: readUtf8(reader) };
    case 6:
      return { type: "binary", value: readLengthPrefixedBytes(reader) };
    case 7: {
      const length = readCollectionLength(reader);
      decodeAssert(
        length <= reader.remaining,
        "change LoroValue list length exceeds input",
      );
      const value: ChangeLoroValue[] = [];
      for (let index = 0; index < length; index += 1) {
        value.push(readChangeLoroValue(reader, depth + 1));
      }
      return { type: "list", value };
    }
    case 8: {
      const length = readCollectionLength(reader);
      decodeAssert(
        length <= Math.floor(reader.remaining / 2),
        "change LoroValue map length exceeds input",
      );
      const value: [bigint, ChangeLoroValue][] = [];
      for (let index = 0; index < length; index += 1) {
        value.push([readUleb128(reader), readChangeLoroValue(reader, depth + 1)]);
      }
      return { type: "map", value };
    }
    case 9:
      return { type: "container-type", value: reader.readU8() };
    default:
      throw new LoroDecodeError("invalid change LoroValue kind", reader.position - 1);
  }
}

export function writeChangeLoroValue(
  writer: ByteWriter,
  value: ChangeLoroValue,
  depth = 0,
): void {
  if (depth > MAX_VALUE_DEPTH) {
    throw new LoroEncodeError("change LoroValue is too deep");
  }
  switch (value.type) {
    case "null":
      writer.writeU8(0);
      return;
    case "bool":
      writer.writeU8(value.value ? 1 : 2);
      return;
    case "i64":
      writer.writeU8(3);
      writeSleb128(writer, value.value);
      return;
    case "double":
      writer.writeU8(4);
      writeF64BE(writer, value.value);
      return;
    case "string":
      writer.writeU8(5);
      writeUtf8(writer, value.value);
      return;
    case "binary":
      writer.writeU8(6);
      writeLengthPrefixedBytes(writer, value.value);
      return;
    case "list":
      writer.writeU8(7);
      assertCollectionLength(value.value.length);
      writeUleb128(writer, value.value.length);
      for (const item of value.value) {
        writeChangeLoroValue(writer, item, depth + 1);
      }
      return;
    case "map":
      writer.writeU8(8);
      assertCollectionLength(value.value.length);
      writeUleb128(writer, value.value.length);
      for (const [keyIndex, item] of value.value) {
        writeUleb128(writer, keyIndex);
        writeChangeLoroValue(writer, item, depth + 1);
      }
      return;
    case "container-type":
      writer.writeU8(9);
      writer.writeU8(value.value);
  }
}

function readCollectionLength(reader: ByteReader): number {
  const value = readUleb128(reader, BigInt(MAX_COLLECTION_LENGTH));
  return Number(value);
}

function readNonnegativeI32(reader: ByteReader, label: string): number {
  const value = readUleb128(reader, 0x7fff_ffffn);
  decodeAssert(value <= 0x7fff_ffffn, `${label} is out of range`);
  return Number(value);
}

function readUtf8(reader: ByteReader): string {
  const offset = reader.position;
  const bytes = readLengthPrefixedBytes(reader);
  try {
    return textDecoder.decode(bytes);
  } catch {
    throw new LoroDecodeError("invalid UTF-8 change value", offset);
  }
}

function writeUtf8(writer: ByteWriter, value: string): void {
  writeLengthPrefixedBytes(writer, textEncoder.encode(value));
}

function readLengthPrefixedBytes(reader: ByteReader): Uint8Array {
  const length = readUleb128(reader, 0x7fff_ffffn);
  return reader.readBytes(Number(length));
}

function writeLengthPrefixedBytes(writer: ByteWriter, value: Uint8Array): void {
  writeUleb128(writer, value.length);
  writer.writeBytes(value);
}

function readF64BE(reader: ByteReader): number {
  const bytes = reader.readBytes(8);
  return new DataView(bytes.buffer, bytes.byteOffset, 8).getFloat64(0, false);
}

function writeF64BE(writer: ByteWriter, value: number): void {
  const bytes = new Uint8Array(8);
  new DataView(bytes.buffer).setFloat64(0, value, false);
  writer.writeBytes(bytes);
}

function assertI32(value: number, label: string): void {
  if (!Number.isSafeInteger(value) || value < -0x8000_0000 || value > 0x7fff_ffff) {
    throw new LoroEncodeError(`${label} is out of range: ${value}`);
  }
}

function assertNonnegativeI32(value: number, label: string): number {
  if (!Number.isSafeInteger(value) || value < 0 || value > 0x7fff_ffff) {
    throw new LoroEncodeError(`${label} is out of range: ${value}`);
  }
  return value;
}

function assertCollectionLength(length: number): void {
  if (!Number.isSafeInteger(length) || length < 0 || length > MAX_COLLECTION_LENGTH) {
    throw new LoroEncodeError(`change value collection is too large: ${length}`);
  }
}
