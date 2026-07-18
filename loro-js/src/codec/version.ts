import { decodeAssert } from "./errors";
import { assertId, readPostcardId, writePostcardId } from "./id";
import { PostcardReader, PostcardWriter } from "./postcard";
import type { Frontiers, Id, VersionVector } from "./types";

export function readPostcardVersionVector(reader: PostcardReader): Id[] {
  const length = reader.readUsize();
  decodeAssert(
    length <= Math.floor(reader.input.remaining / 2),
    "postcard version vector length exceeds remaining input",
    reader.input.position,
  );
  const version: Id[] = [];
  for (let index = 0; index < length; index += 1) {
    version.push(readPostcardId(reader));
  }
  return version;
}

export function writePostcardVersionVector(
  writer: PostcardWriter,
  version: VersionVector,
): void {
  writer.writeUsize(version.length);
  for (const id of version) {
    writePostcardId(writer, id);
  }
}

export function decodePostcardVersionVector(bytes: Uint8Array): Id[] {
  const reader = new PostcardReader(bytes);
  const version = readPostcardVersionVector(reader);
  reader.assertEnd();
  return version;
}

export function encodePostcardVersionVector(version: VersionVector): Uint8Array {
  const writer = new PostcardWriter();
  writePostcardVersionVector(writer, version);
  return writer.toUint8Array();
}

export function readPostcardFrontiers(reader: PostcardReader): Id[] {
  return readPostcardVersionVector(reader);
}

export function writePostcardFrontiers(
  writer: PostcardWriter,
  frontiers: Frontiers,
): void {
  const sorted = [...frontiers];
  for (const id of sorted) {
    assertId(id);
  }
  sorted.sort(compareIds);
  writePostcardVersionVector(writer, sorted);
}

export function decodePostcardFrontiers(bytes: Uint8Array): Id[] {
  const reader = new PostcardReader(bytes);
  const frontiers = readPostcardFrontiers(reader);
  reader.assertEnd();
  return frontiers;
}

export function encodePostcardFrontiers(frontiers: Frontiers): Uint8Array {
  const writer = new PostcardWriter();
  writePostcardFrontiers(writer, frontiers);
  return writer.toUint8Array();
}

function compareIds(left: Id, right: Id): number {
  if (left.peer < right.peer) {
    return -1;
  }
  if (left.peer > right.peer) {
    return 1;
  }
  return left.counter - right.counter;
}
