import { ByteReader, ByteWriter, bytesEqual, compareBytes } from "./bytes";
import { LoroDecodeError, LoroEncodeError, decodeAssert, encodeAssert } from "./errors";
import { decodeLz4Frame, encodeLz4FrameRaw } from "./lz4";
import { LORO_XXHASH_SEED, xxhash32 } from "./xxhash32";

const SSTABLE_MAGIC = Uint8Array.of(0x4c, 0x4f, 0x52, 0x4f);
const SSTABLE_SCHEMA = 0;
const MAX_BLOCK_COUNT = 10_000_000;

export interface SstableEntry {
  readonly key: Uint8Array;
  readonly value: Uint8Array;
}

export type SstableCompression = "none" | "auto" | "lz4";

export interface DecodeSstableOptions {
  readonly checkChecksum?: boolean;
}

export interface EncodeSstableOptions {
  readonly blockSize?: number;
  readonly compression?: SstableCompression;
}

interface BlockMetadata {
  readonly offset: number;
  readonly large: boolean;
  readonly compression: 0 | 1;
  readonly firstKey: Uint8Array;
  readonly lastKey: Uint8Array | undefined;
}

interface EncodedBlock {
  readonly bytes: Uint8Array;
  readonly large: boolean;
  readonly compression: 0 | 1;
  readonly firstKey: Uint8Array;
  readonly lastKey: Uint8Array | undefined;
}

export function decodeSstable(
  bytes: Uint8Array,
  options: DecodeSstableOptions = {},
): SstableEntry[] {
  if (bytes.length === 0) {
    return [];
  }
  decodeAssert(bytes.length >= 17, "SSTable is too short", 0);
  decodeAssert(
    bytesEqual(bytes.subarray(0, 4), SSTABLE_MAGIC),
    "invalid SSTable magic",
    0,
  );
  decodeAssert(bytes[4] === SSTABLE_SCHEMA, "unsupported SSTable schema", 4);
  const footer = new ByteReader(bytes, bytes.length - 4, 4);
  const metadataOffset = footer.readU32LE();
  decodeAssert(
    metadataOffset >= 5 && metadataOffset < bytes.length - 4,
    "invalid SSTable metadata offset",
    bytes.length - 4,
  );
  const metadataBytes = bytes.subarray(metadataOffset, bytes.length - 4);
  const metadata = decodeMetadata(metadataBytes, options.checkChecksum !== false);
  const entries: SstableEntry[] = [];
  for (let index = 0; index < metadata.length; index += 1) {
    const current = metadata[index]!;
    const end =
      index + 1 < metadata.length ? metadata[index + 1]!.offset : metadataOffset;
    decodeAssert(current.offset >= 5, "invalid SSTable block offset", current.offset);
    decodeAssert(
      end > current.offset && end <= metadataOffset,
      "invalid SSTable block range",
      current.offset,
    );
    const stored = bytes.subarray(current.offset, end);
    decodeAssert(stored.length >= 4, "SSTable block lacks checksum", current.offset);
    const payload = stored.subarray(0, stored.length - 4);
    const checksum = new ByteReader(stored, stored.length - 4, 4).readU32LE();
    if (options.checkChecksum !== false) {
      decodeAssert(
        checksum === xxhash32(payload, LORO_XXHASH_SEED),
        "SSTable block checksum mismatch",
        end - 4,
      );
    }
    const decoded =
      current.compression === 0 ? payload : decodeLz4Frame(payload, options);
    if (current.large) {
      entries.push({ key: current.firstKey, value: decoded });
      continue;
    }
    const blockEntries = decodeNormalBlock(decoded, current);
    entries.push(...blockEntries);
  }
  validateEntryOrder(entries);
  return entries;
}

export function encodeSstable(
  input: readonly SstableEntry[],
  options: EncodeSstableOptions = {},
): Uint8Array {
  if (input.length === 0) {
    return new Uint8Array();
  }
  const blockSize = options.blockSize ?? 4096;
  const compression = options.compression ?? "auto";
  if (!Number.isSafeInteger(blockSize) || blockSize <= 0 || blockSize > 0xffff) {
    throw new LoroEncodeError(`invalid SSTable block size ${blockSize}`);
  }
  // Sorting only needs to reorder references; keys and values are copied into
  // the output while encoding and never retained past the returned bytes.
  const entries = input.slice();
  entries.sort((left, right) => compareBytes(left.key, right.key));
  validateEntriesForEncoding(entries);
  const blocks: EncodedBlock[] = [];
  for (let index = 0; index < entries.length; ) {
    const first = entries[index]!;
    if (first.value.length > blockSize || first.value.length > 0xffff) {
      blocks.push(
        encodeStoredBlock(first.value, true, first.key, undefined, compression),
      );
      index += 1;
      continue;
    }
    const data = new ByteWriter();
    const offsets: number[] = [0];
    const firstKey = first.key;
    let lastKey = firstKey;
    data.writeBytes(first.value);
    index += 1;
    while (index < entries.length) {
      const next = entries[index]!;
      if (next.value.length > blockSize || next.value.length > 0xffff) {
        break;
      }
      const common = commonPrefixLength(firstKey, next.key);
      const suffixLength = next.key.length - common;
      if (suffixLength > 0xffff || data.length > 0xffff) {
        break;
      }
      const added = 3 + suffixLength + next.value.length;
      const prospective = data.length + added + (offsets.length + 1) * 2 + 2;
      if (prospective > blockSize) {
        break;
      }
      offsets.push(data.length);
      data.writeU8(common);
      data.writeU16LE(suffixLength);
      data.writeBytes(next.key.subarray(common));
      data.writeBytes(next.value);
      lastKey = next.key;
      index += 1;
    }
    const body = new ByteWriter(data.length + offsets.length * 2 + 2);
    body.writeBytes(data.toUint8Array());
    for (const offset of offsets) {
      body.writeU16LE(offset);
    }
    body.writeU16LE(offsets.length);
    blocks.push(
      encodeStoredBlock(body.toUint8Array(), false, firstKey, lastKey, compression),
    );
  }
  return encodeTable(blocks);
}

function decodeMetadata(bytes: Uint8Array, checkChecksum: boolean): BlockMetadata[] {
  decodeAssert(bytes.length >= 8, "SSTable metadata is too short");
  const reader = new ByteReader(bytes);
  const count = reader.readU32LE();
  decodeAssert(count > 0 && count <= MAX_BLOCK_COUNT, "invalid SSTable block count", 0);
  if (checkChecksum) {
    const stored = new ByteReader(bytes, bytes.length - 4, 4).readU32LE();
    const expected = xxhash32(bytes.subarray(4, bytes.length - 4), LORO_XXHASH_SEED);
    decodeAssert(
      stored === expected,
      "SSTable metadata checksum mismatch",
      bytes.length - 4,
    );
  }
  const metadata: BlockMetadata[] = [];
  for (let index = 0; index < count; index += 1) {
    const offset = reader.readU32LE();
    const firstKey = reader.readBytes(reader.readU16LE());
    const flags = reader.readU8();
    const large = (flags & 0x80) !== 0;
    const compression = flags & 0x7f;
    decodeAssert(
      compression === 0 || compression === 1,
      "invalid SSTable compression tag",
      reader.position - 1,
    );
    const lastKey = large ? undefined : reader.readBytes(reader.readU16LE());
    metadata.push({ offset, large, compression, firstKey, lastKey });
  }
  reader.readU32LE();
  reader.assertEnd("trailing SSTable metadata bytes");
  for (let index = 0; index < metadata.length; index += 1) {
    const current = metadata[index]!;
    decodeAssert(current.firstKey.length > 0, "empty SSTable metadata key");
    if (index > 0) {
      decodeAssert(
        current.offset > metadata[index - 1]!.offset,
        "SSTable block offsets are not strictly increasing",
      );
      decodeAssert(
        compareBytes(metadata[index - 1]!.firstKey, current.firstKey) < 0,
        "SSTable metadata keys are not strictly increasing",
      );
    }
  }
  return metadata;
}

function decodeNormalBlock(bytes: Uint8Array, metadata: BlockMetadata): SstableEntry[] {
  decodeAssert(bytes.length >= 2, "normal SSTable block is too short");
  const count = new ByteReader(bytes, bytes.length - 2, 2).readU16LE();
  decodeAssert(count > 0, "normal SSTable block is empty");
  const offsetsLength = count * 2;
  const dataEnd = bytes.length - offsetsLength - 2;
  decodeAssert(dataEnd >= 0, "normal SSTable offsets exceed block length");
  const offsetsReader = new ByteReader(bytes, dataEnd, offsetsLength);
  const offsets: number[] = [];
  for (let index = 0; index < count; index += 1) {
    offsets.push(offsetsReader.readU16LE());
  }
  decodeAssert(offsets[0] === 0, "first normal SSTable offset is not zero");
  const entries: SstableEntry[] = [];
  for (let index = 0; index < count; index += 1) {
    const start = offsets[index]!;
    const end = index + 1 < count ? offsets[index + 1]! : dataEnd;
    decodeAssert(start <= end && end <= dataEnd, "invalid normal SSTable entry range");
    const entry = bytes.subarray(start, end);
    if (index === 0) {
      entries.push({ key: metadata.firstKey, value: entry });
      continue;
    }
    const reader = new ByteReader(entry);
    const common = reader.readU8();
    decodeAssert(common <= metadata.firstKey.length, "invalid SSTable key prefix length");
    const suffix = reader.readBytes(reader.readU16LE());
    const key = new Uint8Array(common + suffix.length);
    key.set(metadata.firstKey.subarray(0, common));
    key.set(suffix, common);
    entries.push({ key, value: reader.readRemaining() });
  }
  validateEntryOrder(entries);
  const last = entries[entries.length - 1]!;
  decodeAssert(
    metadata.lastKey !== undefined && bytesEqual(last.key, metadata.lastKey),
    "SSTable metadata last key mismatch",
  );
  return entries;
}

function encodeStoredBlock(
  raw: Uint8Array,
  large: boolean,
  firstKey: Uint8Array,
  lastKey: Uint8Array | undefined,
  compression: SstableCompression,
): EncodedBlock {
  const lz4 = compression === "none" ? undefined : encodeLz4FrameRaw(raw);
  const useLz4 =
    compression === "lz4" || (compression === "auto" && lz4!.length <= raw.length);
  const payload = useLz4 ? lz4! : raw;
  const writer = new ByteWriter(payload.length + 4);
  writer.writeBytes(payload);
  writer.writeU32LE(xxhash32(payload, LORO_XXHASH_SEED));
  return {
    bytes: writer.toUint8Array(),
    large,
    compression: useLz4 ? 1 : 0,
    firstKey,
    lastKey,
  };
}

function encodeTable(blocks: readonly EncodedBlock[]): Uint8Array {
  encodeAssert(
    blocks.length > 0 && blocks.length <= MAX_BLOCK_COUNT,
    "invalid SSTable block count",
  );
  let offset = 5;
  const metadata: BlockMetadata[] = [];
  for (const block of blocks) {
    metadata.push({
      offset,
      large: block.large,
      compression: block.compression,
      firstKey: block.firstKey,
      lastKey: block.lastKey,
    });
    offset += block.bytes.length;
    encodeAssert(offset <= 0xffff_ffff, "SSTable exceeds u32 offsets");
  }
  const metadataOffset = offset;
  const metadataWriter = new ByteWriter();
  metadataWriter.writeU32LE(metadata.length);
  for (const item of metadata) {
    encodeAssert(item.firstKey.length <= 0xffff, "SSTable key exceeds u16 length");
    metadataWriter.writeU32LE(item.offset);
    metadataWriter.writeU16LE(item.firstKey.length);
    metadataWriter.writeBytes(item.firstKey);
    metadataWriter.writeU8((item.large ? 0x80 : 0) | item.compression);
    if (!item.large) {
      encodeAssert(item.lastKey !== undefined, "normal SSTable block lacks last key");
      encodeAssert(item.lastKey.length <= 0xffff, "SSTable key exceeds u16 length");
      metadataWriter.writeU16LE(item.lastKey.length);
      metadataWriter.writeBytes(item.lastKey);
    }
  }
  const metadataWithoutChecksum = metadataWriter.toUint8Array();
  metadataWriter.writeU32LE(
    xxhash32(metadataWithoutChecksum.subarray(4), LORO_XXHASH_SEED),
  );
  const writer = new ByteWriter();
  writer.writeBytes(SSTABLE_MAGIC);
  writer.writeU8(SSTABLE_SCHEMA);
  for (const block of blocks) {
    writer.writeBytes(block.bytes);
  }
  writer.writeBytes(metadataWriter.toUint8Array());
  writer.writeU32LE(metadataOffset);
  return writer.toUint8Array();
}

function commonPrefixLength(a: Uint8Array, b: Uint8Array): number {
  const length = Math.min(255, a.length, b.length);
  let index = 0;
  while (index < length && a[index] === b[index]) {
    index += 1;
  }
  return index;
}

function validateEntriesForEncoding(entries: readonly SstableEntry[]): void {
  for (let index = 0; index < entries.length; index += 1) {
    const current = entries[index]!;
    if (current.key.length === 0) {
      throw new LoroEncodeError("SSTable keys cannot be empty");
    }
    if (current.key.length > 0xffff) {
      throw new LoroEncodeError("SSTable key exceeds u16 length");
    }
    if (index > 0 && compareBytes(entries[index - 1]!.key, current.key) >= 0) {
      throw new LoroEncodeError("SSTable keys must be unique");
    }
  }
}

function validateEntryOrder(entries: readonly SstableEntry[]): void {
  for (let index = 1; index < entries.length; index += 1) {
    if (compareBytes(entries[index - 1]!.key, entries[index]!.key) >= 0) {
      throw new LoroDecodeError("SSTable keys are not strictly increasing");
    }
  }
}
