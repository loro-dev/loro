import { readPostcardContainerId, writePostcardContainerId } from "./container-id";
import { decodeAssert } from "./errors";
import { PostcardReader, PostcardWriter } from "./postcard";
const MAX_VALUE_DEPTH = 1024;
export function readPostcardValue(reader, depth = 0) {
    decodeAssert(depth <= MAX_VALUE_DEPTH, "postcard LoroValue is too deep", reader.input.position);
    const tag = reader.readU32();
    switch (tag) {
        case 0:
            return { type: "null" };
        case 1:
            return { type: "bool", value: reader.readBool() };
        case 2:
            return { type: "double", value: reader.readF64() };
        case 3:
            return { type: "i64", value: reader.readI64() };
        case 4:
            return { type: "string", value: reader.readString() };
        case 5: {
            const length = reader.readUsize();
            decodeAssert(length <= reader.input.remaining, "postcard LoroValue list length exceeds remaining input", reader.input.position);
            const value = [];
            for (let index = 0; index < length; index += 1) {
                value.push(readPostcardValue(reader, depth + 1));
            }
            return { type: "list", value };
        }
        case 6: {
            const length = reader.readUsize();
            decodeAssert(length <= Math.floor(reader.input.remaining / 2), "postcard LoroValue map length exceeds remaining input", reader.input.position);
            const value = [];
            for (let index = 0; index < length; index += 1) {
                value.push([reader.readString(), readPostcardValue(reader, depth + 1)]);
            }
            return { type: "map", value };
        }
        case 7:
            return { type: "container", value: readPostcardContainerId(reader) };
        case 8:
            return { type: "binary", value: reader.readBytes() };
        default:
            decodeAssert(false, "invalid postcard LoroValue discriminant", reader.input.position);
    }
}
export function writePostcardValue(writer, value, depth = 0) {
    if (depth > MAX_VALUE_DEPTH) {
        throw new RangeError("postcard LoroValue is too deep");
    }
    switch (value.type) {
        case "null":
            writer.writeU32(0);
            return;
        case "bool":
            writer.writeU32(1);
            writer.writeBool(value.value);
            return;
        case "double":
            writer.writeU32(2);
            writer.writeF64(value.value);
            return;
        case "i64":
            writer.writeU32(3);
            writer.writeI64(value.value);
            return;
        case "string":
            writer.writeU32(4);
            writer.writeString(value.value);
            return;
        case "list":
            writer.writeU32(5);
            writer.writeUsize(value.value.length);
            for (const item of value.value) {
                writePostcardValue(writer, item, depth + 1);
            }
            return;
        case "map":
            writer.writeU32(6);
            writer.writeUsize(value.value.length);
            for (const [key, item] of value.value) {
                writer.writeString(key);
                writePostcardValue(writer, item, depth + 1);
            }
            return;
        case "container":
            writer.writeU32(7);
            writePostcardContainerId(writer, value.value);
            return;
        case "binary":
            writer.writeU32(8);
            writer.writeBytes(value.value);
    }
}
export function decodePostcardValue(bytes) {
    const reader = new PostcardReader(bytes);
    const value = readPostcardValue(reader);
    reader.assertEnd();
    return value;
}
export function encodePostcardValue(value) {
    const writer = new PostcardWriter();
    writePostcardValue(writer, value);
    return writer.toUint8Array();
}
export function readPostcardValues(reader) {
    const length = reader.readUsize();
    decodeAssert(length <= reader.input.remaining, "postcard LoroValue vector length exceeds remaining input", reader.input.position);
    const values = [];
    for (let index = 0; index < length; index += 1) {
        values.push(readPostcardValue(reader));
    }
    return values;
}
export function writePostcardValues(writer, values) {
    writer.writeUsize(values.length);
    for (const value of values) {
        writePostcardValue(writer, value);
    }
}
export function decodePostcardValues(bytes) {
    const reader = new PostcardReader(bytes);
    const values = readPostcardValues(reader);
    reader.assertEnd();
    return values;
}
export function encodePostcardValues(values) {
    const writer = new PostcardWriter();
    writePostcardValues(writer, values);
    return writer.toUint8Array();
}
export function readPostcardValueMap(reader) {
    const length = reader.readUsize();
    decodeAssert(length <= Math.floor(reader.input.remaining / 2), "postcard LoroValue map length exceeds remaining input", reader.input.position);
    const entries = [];
    for (let index = 0; index < length; index += 1) {
        entries.push([reader.readString(), readPostcardValue(reader)]);
    }
    return entries;
}
export function writePostcardValueMap(writer, entries) {
    writer.writeUsize(entries.length);
    for (const [key, value] of entries) {
        writer.writeString(key);
        writePostcardValue(writer, value);
    }
}
export function decodePostcardValueMap(bytes) {
    const reader = new PostcardReader(bytes);
    const entries = readPostcardValueMap(reader);
    reader.assertEnd();
    return entries;
}
export function encodePostcardValueMap(entries) {
    const writer = new PostcardWriter();
    writePostcardValueMap(writer, entries);
    return writer.toUint8Array();
}
