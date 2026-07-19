import { ByteReader, ByteWriter } from "./bytes";
import { LoroDecodeError, LoroEncodeError } from "./errors";
import { PostcardReader, PostcardWriter } from "./postcard";
const I32_MIN = -2147483648;
const I32_MAX = 2147483647;
const U64_MAX = 0xffffffffffffffffn;
export function decodeChangeBlockKey(bytes) {
    if (bytes.length !== 12) {
        throw new LoroDecodeError("invalid change block key length");
    }
    const reader = new ByteReader(bytes);
    const peer = reader.readU64BE();
    const counter = reader.readU32BE() | 0;
    reader.assertEnd("trailing change block key bytes");
    return { peer, counter };
}
export function encodeChangeBlockKey(id) {
    assertId(id);
    const writer = new ByteWriter(12);
    writer.writeU64BE(id.peer);
    writer.writeU32BE(id.counter >>> 0);
    return writer.toUint8Array();
}
export function readPostcardId(reader) {
    return {
        peer: reader.readU64(),
        counter: reader.readI32(),
    };
}
export function writePostcardId(writer, id) {
    assertId(id);
    writer.writeU64(id.peer);
    writer.writeI32(id.counter);
}
export function decodePostcardId(bytes) {
    const reader = new PostcardReader(bytes);
    const id = readPostcardId(reader);
    reader.assertEnd();
    return id;
}
export function encodePostcardId(id) {
    const writer = new PostcardWriter();
    writePostcardId(writer, id);
    return writer.toUint8Array();
}
export function assertId(id) {
    if (id.peer < 0n || id.peer > U64_MAX) {
        throw new LoroEncodeError(`peer ID is out of range: ${id.peer}`);
    }
    if (!Number.isSafeInteger(id.counter) || id.counter < I32_MIN || id.counter > I32_MAX) {
        throw new LoroEncodeError(`ID counter is out of range: ${id.counter}`);
    }
}
