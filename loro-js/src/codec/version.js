import { decodeAssert } from "./errors";
import { assertId, readPostcardId, writePostcardId } from "./id";
import { PostcardReader, PostcardWriter } from "./postcard";
export function readPostcardVersionVector(reader) {
    const length = reader.readUsize();
    decodeAssert(length <= Math.floor(reader.input.remaining / 2), "postcard version vector length exceeds remaining input", reader.input.position);
    const version = [];
    for (let index = 0; index < length; index += 1) {
        version.push(readPostcardId(reader));
    }
    return version;
}
export function writePostcardVersionVector(writer, version) {
    writer.writeUsize(version.length);
    for (const id of version) {
        writePostcardId(writer, id);
    }
}
export function decodePostcardVersionVector(bytes) {
    const reader = new PostcardReader(bytes);
    const version = readPostcardVersionVector(reader);
    reader.assertEnd();
    return version;
}
export function encodePostcardVersionVector(version) {
    const writer = new PostcardWriter();
    writePostcardVersionVector(writer, version);
    return writer.toUint8Array();
}
export function readPostcardFrontiers(reader) {
    return readPostcardVersionVector(reader);
}
export function writePostcardFrontiers(writer, frontiers) {
    const sorted = [...frontiers];
    for (const id of sorted) {
        assertId(id);
    }
    sorted.sort(compareIds);
    writePostcardVersionVector(writer, sorted);
}
export function decodePostcardFrontiers(bytes) {
    const reader = new PostcardReader(bytes);
    const frontiers = readPostcardFrontiers(reader);
    reader.assertEnd();
    return frontiers;
}
export function encodePostcardFrontiers(frontiers) {
    const writer = new PostcardWriter();
    writePostcardFrontiers(writer, frontiers);
    return writer.toUint8Array();
}
function compareIds(left, right) {
    if (left.peer < right.peer) {
        return -1;
    }
    if (left.peer > right.peer) {
        return 1;
    }
    return left.counter - right.counter;
}
