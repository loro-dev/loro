import { ByteReader, ByteWriter, bytesEqual } from "./bytes";
import { LoroDecodeError, LoroEncodeError, decodeAssert } from "./errors";
import { readUlebNumber, writeUleb128 } from "./leb128";
import { LORO_XXHASH_SEED, xxhash32 } from "./xxhash32";

const DOCUMENT_MAGIC = Uint8Array.of(0x6c, 0x6f, 0x72, 0x6f);
const DOCUMENT_HEADER_LENGTH = 22;

export enum EncodeMode {
  FastSnapshot = 3,
  FastUpdates = 4,
}

export interface ParsedDocument {
  readonly mode: EncodeMode;
  readonly body: Uint8Array;
  readonly checksum: number;
  readonly checksumPrefix: Uint8Array;
}

export interface FastSnapshotBody {
  readonly oplog: Uint8Array;
  readonly state: Uint8Array;
  readonly shallowRootState: Uint8Array;
}

export interface DecodeDocumentOptions {
  readonly checkChecksum?: boolean;
  readonly requireCanonicalPrefix?: boolean;
}

export function decodeDocument(
  bytes: Uint8Array,
  options: DecodeDocumentOptions = {},
): ParsedDocument {
  const reader = new ByteReader(bytes);
  decodeAssert(bytes.length >= DOCUMENT_HEADER_LENGTH, "document is too short", 0);
  decodeAssert(
    bytesEqual(reader.readBytes(4), DOCUMENT_MAGIC),
    "invalid document magic",
    0,
  );
  const checksumPrefix = reader.readBytes(12);
  if (options.requireCanonicalPrefix === true) {
    for (const byte of checksumPrefix) {
      decodeAssert(byte === 0, "nonzero document checksum prefix", 4);
    }
  }
  const checksum = reader.readU32LE();
  const rawMode = reader.readU16BE();
  if (rawMode !== EncodeMode.FastSnapshot && rawMode !== EncodeMode.FastUpdates) {
    throw new LoroDecodeError(`unsupported document mode ${rawMode}`, 20);
  }
  const body = reader.readRemaining();
  if (options.checkChecksum !== false) {
    const expected = xxhash32(bytes.subarray(20), LORO_XXHASH_SEED);
    decodeAssert(checksum === expected, "document checksum mismatch", 16);
  }
  return { mode: rawMode, body, checksum, checksumPrefix };
}

export function encodeDocument(mode: EncodeMode, body: Uint8Array): Uint8Array {
  if (mode !== EncodeMode.FastSnapshot && mode !== EncodeMode.FastUpdates) {
    throw new LoroEncodeError(`unsupported document mode ${mode as number}`);
  }
  const output = new Uint8Array(DOCUMENT_HEADER_LENGTH + body.length);
  output.set(DOCUMENT_MAGIC, 0);
  output[20] = mode >>> 8;
  output[21] = mode & 0xff;
  output.set(body, DOCUMENT_HEADER_LENGTH);
  new DataView(output.buffer, output.byteOffset, output.byteLength).setUint32(
    16,
    xxhash32(output.subarray(20), LORO_XXHASH_SEED),
    true,
  );
  return output;
}

export function decodeFastSnapshotBody(body: Uint8Array): FastSnapshotBody {
  const reader = new ByteReader(body);
  const oplog = readU32LengthPrefixed(reader, "oplog");
  const state = readU32LengthPrefixed(reader, "state");
  const shallowRootState = readU32LengthPrefixed(reader, "shallow root state");
  reader.assertEnd("trailing FastSnapshot bytes");
  return { oplog, state, shallowRootState };
}

export function encodeFastSnapshotBody(snapshot: FastSnapshotBody): Uint8Array {
  for (const [value, label] of [
    [snapshot.oplog, "oplog"],
    [snapshot.state, "state"],
    [snapshot.shallowRootState, "shallow root state"],
  ] as const) {
    if (value.length > 0xffff_ffff) {
      throw new LoroEncodeError(`${label} is too large`);
    }
  }
  const output = new Uint8Array(
    12 + snapshot.oplog.length + snapshot.state.length + snapshot.shallowRootState.length,
  );
  const view = new DataView(output.buffer, output.byteOffset, output.byteLength);
  let offset = 0;
  for (const value of [snapshot.oplog, snapshot.state, snapshot.shallowRootState]) {
    view.setUint32(offset, value.length, true);
    offset += 4;
    output.set(value, offset);
    offset += value.length;
  }
  return output;
}

export function decodeFastUpdatesBody(body: Uint8Array): Uint8Array[] {
  const reader = new ByteReader(body);
  const blocks: Uint8Array[] = [];
  while (reader.remaining > 0) {
    const length = readUlebNumber(reader, 0xffff_ffff);
    blocks.push(reader.readBytes(length));
  }
  return blocks;
}

export function encodeFastUpdatesBody(blocks: readonly Uint8Array[]): Uint8Array {
  const length = blocks.reduce(
    (sum, block) => sum + ulebByteLength(block.length) + block.length,
    0,
  );
  const writer = new ByteWriter(length);
  for (const block of blocks) {
    writeUleb128(writer, block.length);
    writer.writeBytes(block);
  }
  return writer.toUint8Array();
}

function ulebByteLength(value: number): number {
  let length = 1;
  while (value >= 0x80) {
    value = Math.floor(value / 0x80);
    length += 1;
  }
  return length;
}

export function decodeFastSnapshot(
  bytes: Uint8Array,
  options?: DecodeDocumentOptions,
): FastSnapshotBody {
  const document = decodeDocument(bytes, options);
  decodeAssert(
    document.mode === EncodeMode.FastSnapshot,
    "document is not a FastSnapshot",
    20,
  );
  return decodeFastSnapshotBody(document.body);
}

export function encodeFastSnapshot(snapshot: FastSnapshotBody): Uint8Array {
  return encodeDocument(EncodeMode.FastSnapshot, encodeFastSnapshotBody(snapshot));
}

export function decodeFastUpdates(
  bytes: Uint8Array,
  options?: DecodeDocumentOptions,
): Uint8Array[] {
  const document = decodeDocument(bytes, options);
  decodeAssert(
    document.mode === EncodeMode.FastUpdates,
    "document is not FastUpdates",
    20,
  );
  return decodeFastUpdatesBody(document.body);
}

export function encodeFastUpdates(blocks: readonly Uint8Array[]): Uint8Array {
  const bodyLength = blocks.reduce(
    (sum, block) => sum + ulebByteLength(block.length) + block.length,
    0,
  );
  const output = new Uint8Array(DOCUMENT_HEADER_LENGTH + bodyLength);
  output.set(DOCUMENT_MAGIC, 0);
  output[20] = EncodeMode.FastUpdates >>> 8;
  output[21] = EncodeMode.FastUpdates & 0xff;
  let offset = DOCUMENT_HEADER_LENGTH;
  for (const block of blocks) {
    offset = writeUlebNumber(output, offset, block.length);
    output.set(block, offset);
    offset += block.length;
  }
  new DataView(output.buffer, output.byteOffset, output.byteLength).setUint32(
    16,
    xxhash32(output.subarray(20), LORO_XXHASH_SEED),
    true,
  );
  return output;
}

function writeUlebNumber(output: Uint8Array, offset: number, input: number): number {
  let value = input;
  do {
    let byte = value % 128;
    value = Math.floor(value / 128);
    if (value !== 0) {
      byte |= 0x80;
    }
    output[offset] = byte;
    offset += 1;
  } while (value !== 0);
  return offset;
}

function readU32LengthPrefixed(reader: ByteReader, label: string): Uint8Array {
  const offset = reader.position;
  const length = reader.readU32LE();
  if (length > reader.remaining) {
    throw new LoroDecodeError(`${label} length exceeds remaining input`, offset);
  }
  return reader.readBytes(length);
}
