import { ByteReader, ByteWriter } from "./bytes";
import { LoroDecodeError, LoroEncodeError } from "./errors";
import { PostcardReader, PostcardWriter } from "./postcard";
import type { Id } from "./types";

const I32_MIN = -0x8000_0000;
const I32_MAX = 0x7fff_ffff;
const U64_MAX = 0xffff_ffff_ffff_ffffn;

export function decodeChangeBlockKey(bytes: Uint8Array): Id {
  if (bytes.length !== 12) {
    throw new LoroDecodeError("invalid change block key length");
  }
  const reader = new ByteReader(bytes);
  const peer = reader.readU64BE();
  const counter = reader.readU32BE() | 0;
  reader.assertEnd("trailing change block key bytes");
  return { peer, counter };
}

export function encodeChangeBlockKey(id: Id): Uint8Array {
  assertId(id);
  const writer = new ByteWriter(12);
  writer.writeU64BE(id.peer);
  writer.writeU32BE(id.counter >>> 0);
  return writer.toUint8Array();
}

export function readPostcardId(reader: PostcardReader): Id {
  return {
    peer: reader.readU64(),
    counter: reader.readI32(),
  };
}

export function writePostcardId(writer: PostcardWriter, id: Id): void {
  assertId(id);
  writer.writeU64(id.peer);
  writer.writeI32(id.counter);
}

export function decodePostcardId(bytes: Uint8Array): Id {
  const reader = new PostcardReader(bytes);
  const id = readPostcardId(reader);
  reader.assertEnd();
  return id;
}

export function encodePostcardId(id: Id): Uint8Array {
  const writer = new PostcardWriter();
  writePostcardId(writer, id);
  return writer.toUint8Array();
}

export function assertId(id: Id): void {
  if (id.peer < 0n || id.peer > U64_MAX) {
    throw new LoroEncodeError(`peer ID is out of range: ${id.peer}`);
  }
  if (!Number.isSafeInteger(id.counter) || id.counter < I32_MIN || id.counter > I32_MAX) {
    throw new LoroEncodeError(`ID counter is out of range: ${id.counter}`);
  }
}
