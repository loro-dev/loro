import { ByteReader, ByteWriter } from "./bytes";
import { LoroDecodeError, LoroEncodeError, decodeAssert } from "./errors";
import { xxhash32 } from "./xxhash32";

const LZ4_MAGIC = 0x184d_2204;
const WINDOW_SIZE = 65_536;

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

  const output: number[] = [];
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
      appendBytes(output, block);
    } else {
      const prefixStart = independentBlocks
        ? blockOutputStart
        : Math.max(0, blockOutputStart - WINDOW_SIZE);
      decodeLz4BlockInto(block, output, prefixStart);
    }
    decodeAssert(
      output.length - blockOutputStart <= maxBlockSize,
      "LZ4 data block output exceeds BD maximum",
      infoOffset,
    );
  }

  const result = Uint8Array.from(output);
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
    writer.writeU32LE((0x8000_0000 + block.length) >>> 0);
    writer.writeBytes(block);
  }
  writer.writeU32LE(0);
  return writer.toUint8Array();
}

function decodeLz4BlockInto(
  input: Uint8Array,
  output: number[],
  prefixStart: number,
): void {
  const reader = new ByteReader(input);
  while (reader.remaining > 0) {
    const token = reader.readU8();
    const literalLength = readExtendedLength(reader, token >>> 4);
    appendBytes(output, reader.readBytes(literalLength));
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
    for (let index = 0; index < matchLength; index += 1) {
      const value = output[output.length - offset];
      decodeAssert(
        value !== undefined,
        "LZ4 match source is out of bounds",
        offsetPosition,
      );
      output.push(value);
    }
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

function appendBytes(output: number[], bytes: Uint8Array): void {
  for (const byte of bytes) {
    output.push(byte);
  }
}
