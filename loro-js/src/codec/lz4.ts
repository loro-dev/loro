import { ByteReader, ByteWriter } from "./bytes";
import { LoroDecodeError, LoroEncodeError, decodeAssert } from "./errors";
import { xxhash32 } from "./xxhash32";

const LZ4_MAGIC = 0x184d_2204;
const WINDOW_SIZE = 65_536;
const MAX_MATCH_DISTANCE = 0xffff;
const MIN_MATCH_LENGTH = 4;
const LAST_LITERALS = 5;
const MATCH_FIND_LIMIT = 12;
const HASH_LOG = 16;
const MIN_HASH_LOG = 10;
const HASH_MULTIPLIER = 0x9e37_79b1;

export interface DecodeLz4Options {
  readonly checkChecksum?: boolean;
  readonly requireCanonicalProfile?: boolean;
}

export function decodeLz4Frame(
  bytes: Uint8Array,
  options: DecodeLz4Options = {},
): Uint8Array {
  const reader = new ByteReader(bytes);
  decodeAssert(reader.readU32LE() === LZ4_MAGIC, "invalid LZ4 frame magic", 0);
  const descriptorStart = reader.position;
  const flags = reader.readU8();
  const version = (flags >>> 6) & 0x03;
  decodeAssert(version === 1, "unsupported LZ4 frame version", descriptorStart);
  decodeAssert((flags & 0x02) === 0, "reserved LZ4 FLG bit is set", descriptorStart);
  const independentBlocks = (flags & 0x20) !== 0;
  const blockChecksum = (flags & 0x10) !== 0;
  const hasContentSize = (flags & 0x08) !== 0;
  const contentChecksum = (flags & 0x04) !== 0;
  const hasDictionary = (flags & 0x01) !== 0;
  const blockDescriptor = reader.readU8();
  const maxBlockSize = decodeBlockMaximum(blockDescriptor, reader.position - 1);
  let expectedContentSize: bigint | undefined;
  if (hasContentSize) {
    expectedContentSize = reader.readU64LE();
  }
  if (hasDictionary) {
    reader.readU32LE();
    throw new LoroDecodeError("LZ4 dictionaries are unsupported", descriptorStart);
  }
  const headerChecksumPosition = reader.position;
  const storedHeaderChecksum = reader.readU8();
  if (options.checkChecksum !== false) {
    const descriptor = bytes.subarray(descriptorStart, headerChecksumPosition);
    const expected = (xxhash32(descriptor, 0) >>> 8) & 0xff;
    decodeAssert(
      storedHeaderChecksum === expected,
      "LZ4 header checksum mismatch",
      headerChecksumPosition,
    );
  }
  if (options.requireCanonicalProfile === true) {
    decodeAssert(flags === 0x60, "noncanonical LZ4 frame flags", descriptorStart);
  }

  const output = new Lz4Output();
  for (;;) {
    const infoOffset = reader.position;
    const blockInfo = reader.readU32LE();
    if (blockInfo === 0) {
      break;
    }
    const raw = (blockInfo & 0x8000_0000) !== 0;
    const length = blockInfo & 0x7fff_ffff;
    decodeAssert(length <= maxBlockSize, "LZ4 data block exceeds BD maximum", infoOffset);
    const block = reader.readBytes(length);
    if (blockChecksum) {
      const stored = reader.readU32LE();
      if (options.checkChecksum !== false) {
        decodeAssert(
          stored === xxhash32(block, 0),
          "LZ4 block checksum mismatch",
          reader.position - 4,
        );
      }
    }
    const blockOutputStart = output.length;
    if (raw) {
      output.ensureCapacity(block.length);
      output.writeBytes(block);
    } else {
      output.ensureCapacity(maxBlockSize);
      const prefixStart = independentBlocks
        ? blockOutputStart
        : Math.max(0, blockOutputStart - WINDOW_SIZE);
      decodeLz4BlockInto(block, output, prefixStart, blockOutputStart + maxBlockSize);
    }
    decodeAssert(
      output.length - blockOutputStart <= maxBlockSize,
      "LZ4 data block output exceeds BD maximum",
      infoOffset,
    );
  }

  const result = output.finish();
  if (expectedContentSize !== undefined) {
    decodeAssert(
      expectedContentSize === BigInt(result.length),
      "LZ4 content size mismatch",
      descriptorStart,
    );
  }
  if (contentChecksum) {
    const stored = reader.readU32LE();
    if (options.checkChecksum !== false) {
      decodeAssert(
        stored === xxhash32(result, 0),
        "LZ4 content checksum mismatch",
        reader.position - 4,
      );
    }
  }
  reader.assertEnd("trailing LZ4 frame bytes");
  return result;
}

export function encodeLz4FrameRaw(bytes: Uint8Array): Uint8Array {
  const { descriptor, maxBlockSize } = chooseBlockDescriptor(bytes.length);
  const flags = 0x60;
  const header = Uint8Array.of(flags, descriptor);
  const writer = new ByteWriter(
    bytes.length + 16 + Math.ceil(bytes.length / maxBlockSize) * 4,
  );
  writer.writeU32LE(LZ4_MAGIC);
  writer.writeBytes(header);
  writer.writeU8((xxhash32(header, 0) >>> 8) & 0xff);
  for (let offset = 0; offset < bytes.length; offset += maxBlockSize) {
    const block = bytes.subarray(offset, Math.min(bytes.length, offset + maxBlockSize));
    const compressed = encodeLz4Block(block);
    if (compressed.length < block.length) {
      writer.writeU32LE(compressed.length);
      writer.writeBytes(compressed);
    } else {
      writer.writeU32LE((0x8000_0000 + block.length) >>> 0);
      writer.writeBytes(block);
    }
  }
  writer.writeU32LE(0);
  return writer.toUint8Array();
}

function encodeLz4Block(input: Uint8Array): Uint8Array {
  if (input.length < MATCH_FIND_LIMIT + 1) {
    return encodeLastLiterals(input);
  }

  const writer = new ByteWriter(input.length);
  const hashLog = Math.min(
    HASH_LOG,
    Math.max(MIN_HASH_LOG, Math.ceil(Math.log2(input.length))),
  );
  const hashShift = 32 - hashLog;
  const hashTable = new Uint32Array(1 << hashLog);
  const lastMatchStart = input.length - MATCH_FIND_LIMIT;
  const matchLimit = input.length - LAST_LITERALS;
  let anchor = 0;
  let position = 0;

  while (position <= lastMatchStart) {
    const hash = hashSequence(input, position, hashShift);
    const candidateEntry = hashTable[hash]!;
    hashTable[hash] = position + 1;
    const candidate = candidateEntry - 1;

    if (
      candidateEntry === 0 ||
      position - candidate > MAX_MATCH_DISTANCE ||
      !equalFourBytes(input, candidate, position)
    ) {
      position += 1;
      continue;
    }

    let matchEnd = position + MIN_MATCH_LENGTH;
    let candidateEnd = candidate + MIN_MATCH_LENGTH;
    while (matchEnd < matchLimit && input[candidateEnd] === input[matchEnd]) {
      matchEnd += 1;
      candidateEnd += 1;
    }

    writeSequence(
      writer,
      input.subarray(anchor, position),
      position - candidate,
      matchEnd - position,
    );

    for (let index = position + 1; index < matchEnd; index += 1) {
      if (index + MIN_MATCH_LENGTH > input.length) {
        break;
      }
      hashTable[hashSequence(input, index, hashShift)] = index + 1;
    }
    position = matchEnd;
    anchor = matchEnd;
  }

  writeLastLiterals(writer, input.subarray(anchor));
  return writer.toUint8Array();
}

function encodeLastLiterals(input: Uint8Array): Uint8Array {
  const writer = new ByteWriter(input.length + 2);
  writeLastLiterals(writer, input);
  return writer.toUint8Array();
}

function writeSequence(
  writer: ByteWriter,
  literals: Uint8Array,
  offset: number,
  matchLength: number,
): void {
  const encodedMatchLength = matchLength - MIN_MATCH_LENGTH;
  writer.writeU8((Math.min(literals.length, 15) << 4) | Math.min(encodedMatchLength, 15));
  writeExtendedLength(writer, literals.length);
  writer.writeBytes(literals);
  writer.writeU16LE(offset);
  writeExtendedLength(writer, encodedMatchLength);
}

function writeLastLiterals(writer: ByteWriter, literals: Uint8Array): void {
  writer.writeU8(Math.min(literals.length, 15) << 4);
  writeExtendedLength(writer, literals.length);
  writer.writeBytes(literals);
}

function writeExtendedLength(writer: ByteWriter, length: number): void {
  if (length < 15) {
    return;
  }
  let remaining = length - 15;
  while (remaining >= 255) {
    writer.writeU8(255);
    remaining -= 255;
  }
  writer.writeU8(remaining);
}

function hashSequence(input: Uint8Array, offset: number, hashShift: number): number {
  const sequence =
    (input[offset]! |
      (input[offset + 1]! << 8) |
      (input[offset + 2]! << 16) |
      (input[offset + 3]! << 24)) >>>
    0;
  return Math.imul(sequence, HASH_MULTIPLIER) >>> hashShift;
}

function equalFourBytes(input: Uint8Array, left: number, right: number): boolean {
  return (
    input[left] === input[right] &&
    input[left + 1] === input[right + 1] &&
    input[left + 2] === input[right + 2] &&
    input[left + 3] === input[right + 3]
  );
}

function decodeLz4BlockInto(
  input: Uint8Array,
  output: Lz4Output,
  prefixStart: number,
  outputLimit: number,
): void {
  const reader = new ByteReader(input);
  while (reader.remaining > 0) {
    const token = reader.readU8();
    const literalLength = readExtendedLength(reader, token >>> 4);
    decodeAssert(
      literalLength <= outputLimit - output.length,
      "LZ4 data block output exceeds BD maximum",
      reader.position,
    );
    output.writeBytes(reader.readBytes(literalLength));
    if (reader.remaining === 0) {
      return;
    }
    const offsetPosition = reader.position;
    const offset = reader.readU16LE();
    decodeAssert(offset !== 0, "zero LZ4 match offset", offsetPosition);
    decodeAssert(
      offset <= output.length - prefixStart,
      "LZ4 match offset is out of bounds",
      offsetPosition,
    );
    const matchLength = 4 + readExtendedLength(reader, token & 0x0f);
    decodeAssert(
      matchLength <= outputLimit - output.length,
      "LZ4 data block output exceeds BD maximum",
      reader.position,
    );
    for (let index = 0; index < matchLength; index += 1) {
      const value = output.bytes[output.length - offset];
      decodeAssert(
        value !== undefined,
        "LZ4 match source is out of bounds",
        offsetPosition,
      );
      output.bytes[output.length] = value;
      output.length += 1;
    }
  }
}

class Lz4Output {
  bytes = new Uint8Array();
  length = 0;

  ensureCapacity(extra: number): void {
    const required = this.length + extra;
    if (required <= this.bytes.length) {
      return;
    }
    let capacity = Math.max(64, this.bytes.length);
    while (capacity < required) {
      capacity = Math.max(required, capacity * 2);
    }
    const next = new Uint8Array(capacity);
    next.set(this.bytes.subarray(0, this.length));
    this.bytes = next;
  }

  writeBytes(bytes: Uint8Array): void {
    this.ensureCapacity(bytes.length);
    this.bytes.set(bytes, this.length);
    this.length += bytes.length;
  }

  finish(): Uint8Array {
    return this.length === this.bytes.length
      ? this.bytes
      : this.bytes.slice(0, this.length);
  }
}

function readExtendedLength(reader: ByteReader, base: number): number {
  let length = base;
  if (base !== 15) {
    return length;
  }
  for (;;) {
    const extra = reader.readU8();
    length += extra;
    decodeAssert(
      Number.isSafeInteger(length),
      "LZ4 sequence length overflow",
      reader.position - 1,
    );
    if (extra !== 255) {
      return length;
    }
  }
}

function decodeBlockMaximum(descriptor: number, offset: number): number {
  decodeAssert((descriptor & 0x8f) === 0, "invalid LZ4 BD byte", offset);
  switch ((descriptor >>> 4) & 0x07) {
    case 4:
      return 64 * 1024;
    case 5:
      return 256 * 1024;
    case 6:
      return 1024 * 1024;
    case 7:
      return 4 * 1024 * 1024;
    default:
      throw new LoroDecodeError("unsupported LZ4 block maximum", offset);
  }
}

function chooseBlockDescriptor(length: number): {
  descriptor: number;
  maxBlockSize: number;
} {
  if (!Number.isSafeInteger(length) || length < 0) {
    throw new LoroEncodeError("invalid LZ4 input length");
  }
  if (length <= 64 * 1024) {
    return { descriptor: 0x40, maxBlockSize: 64 * 1024 };
  }
  if (length <= 256 * 1024) {
    return { descriptor: 0x50, maxBlockSize: 256 * 1024 };
  }
  if (length <= 1024 * 1024) {
    return { descriptor: 0x60, maxBlockSize: 1024 * 1024 };
  }
  return { descriptor: 0x70, maxBlockSize: 4 * 1024 * 1024 };
}
