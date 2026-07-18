import { ByteReader, ByteWriter } from "./bytes";
import { LoroEncodeError, decodeAssert } from "./errors";
import { readUlebNumber, writeUleb128 } from "./leb128";
import {
  decodeAnyRleUsize,
  decodeColumnarVecMaybeWrapped,
  encodeAnyRleUsize,
  encodeColumnarVecWrapped,
} from "./serde-columnar";

export function decodePositionArena(bytes: Uint8Array): Uint8Array[] {
  if (bytes.length === 0) {
    return [];
  }
  const columns = decodeColumnarVecMaybeWrapped(bytes);
  decodeAssert(columns.length === 2, "position arena must have two columns");
  const commonPrefixes = decodeAnyRleUsize(columns[0]!);
  const reader = new ByteReader(columns[1]!);
  const count = readUlebNumber(reader, 10_000_000);
  decodeAssert(count === commonPrefixes.length, "position arena column length mismatch");
  const positions: Uint8Array[] = [];
  let previous: Uint8Array = new Uint8Array();
  let previousLength = 0;
  for (const commonBigInt of commonPrefixes) {
    // Number comparison is exact here: accepted values are at most
    // previousLength (an array length), so Number(commonBigInt) is precise
    // whenever the assertion passes, and larger BigInts convert to values
    // greater than previousLength and fail the same way.
    const common = Number(commonBigInt);
    decodeAssert(common <= previousLength, "invalid position arena common prefix");
    const suffix = reader.readBytes(readUlebNumber(reader, 0x7fff_ffff));
    const position = new Uint8Array(common + suffix.length);
    position.set(previous.subarray(0, common));
    position.set(suffix, common);
    positions.push(position);
    previous = position;
    previousLength = position.length;
  }
  reader.assertEnd("trailing position arena bytes");
  return positions;
}

export function encodePositionArena(
  positions: readonly Uint8Array[],
  options: { readonly encodeEmpty?: boolean } = {},
): Uint8Array {
  if (positions.length === 0 && options.encodeEmpty !== true) {
    return new Uint8Array();
  }
  if (positions.length > 10_000_000) {
    throw new LoroEncodeError(`position arena is too large: ${positions.length}`);
  }
  const commonPrefixes: bigint[] = [];
  const suffixes = new ByteWriter();
  writeUleb128(suffixes, positions.length);
  let previous: Uint8Array = new Uint8Array();
  for (const position of positions) {
    const common = commonPrefixLength(previous, position);
    commonPrefixes.push(BigInt(common));
    const suffix = position.subarray(common);
    writeUleb128(suffixes, suffix.length);
    suffixes.writeBytes(suffix);
    previous = position;
  }
  return encodeColumnarVecWrapped([
    encodeAnyRleUsize(commonPrefixes),
    suffixes.toUint8Array(),
  ]);
}

function commonPrefixLength(left: Uint8Array, right: Uint8Array): number {
  const length = Math.min(left.length, right.length);
  let index = 0;
  while (index < length && left[index] === right[index]) {
    index += 1;
  }
  return index;
}
