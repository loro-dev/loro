import { ContainerType as CodecContainerType, } from "../codec/types";
const U64_MAX = 0xffffffffffffffffn;
export function parsePeerId(peer) {
    let value;
    try {
        value = typeof peer === "bigint" ? peer : BigInt(peer);
    }
    catch {
        throw new TypeError("peer id must be an unsigned 64-bit integer");
    }
    if (value < 0n || value > U64_MAX) {
        throw new RangeError(`peer id is out of range: ${String(peer)}`);
    }
    return value;
}
export function peerIdToString(peer) {
    return peer.toString();
}
export function parseOpId(id) {
    assertCounter(id.counter);
    return { peer: parsePeerId(id.peer), counter: id.counter };
}
export function formatOpId(id) {
    return { peer: peerIdToString(id.peer), counter: id.counter };
}
export function formatTreeId(id) {
    return `${id.counter}@${id.peer}`;
}
export function idStrToId(id) {
    return formatOpId(parseTreeId(id));
}
export function parseTreeId(id) {
    const separator = id.indexOf("@");
    if (separator <= 0 || id.indexOf("@", separator + 1) !== -1) {
        throw new TypeError(`invalid tree id: ${id}`);
    }
    const counter = Number(id.slice(0, separator));
    assertCounter(counter);
    return { peer: parsePeerId(id.slice(separator + 1)), counter };
}
export function codecTypeToPublic(type) {
    switch (type) {
        case CodecContainerType.Text:
            return "Text";
        case CodecContainerType.Map:
            return "Map";
        case CodecContainerType.List:
            return "List";
        case CodecContainerType.Tree:
            return "Tree";
        case CodecContainerType.MovableList:
            return "MovableList";
        case CodecContainerType.Counter:
            return "Counter";
        default:
            throw new TypeError(`unsupported container type ${type.value}`);
    }
}
export function publicTypeToCodec(type) {
    switch (type) {
        case "Text":
            return CodecContainerType.Text;
        case "Map":
            return CodecContainerType.Map;
        case "List":
            return CodecContainerType.List;
        case "Tree":
            return CodecContainerType.Tree;
        case "MovableList":
            return CodecContainerType.MovableList;
        case "Counter":
            return CodecContainerType.Counter;
    }
}
export function formatContainerId(id) {
    const type = codecTypeToPublic(id.containerType);
    return (id.kind === "root"
        ? `cid:root-${id.name}:${type}`
        : `cid:${id.counter}@${id.peer}:${type}`);
}
export function parseContainerId(id) {
    if (!id.startsWith("cid:")) {
        throw new TypeError(`invalid container id: ${id}`);
    }
    const typeSeparator = id.lastIndexOf(":");
    if (typeSeparator <= 4) {
        throw new TypeError(`invalid container id: ${id}`);
    }
    const type = id.slice(typeSeparator + 1);
    const containerType = publicTypeToCodec(type);
    const body = id.slice(4, typeSeparator);
    if (body.startsWith("root-")) {
        return { kind: "root", name: body.slice(5), containerType };
    }
    const operationId = parseTreeId(body);
    return { kind: "normal", ...operationId, containerType };
}
export function newContainerID(id, type) {
    return formatContainerId({
        kind: "normal",
        ...parseOpId(id),
        containerType: publicTypeToCodec(type),
    });
}
export function newRootContainerID(name, type) {
    return formatContainerId({
        kind: "root",
        name,
        containerType: publicTypeToCodec(type),
    });
}
export function isContainerId(value) {
    // Cheap rejection first: allocating the TypeError (and its stack frames) is
    // the dominant cost when this guards arbitrary strings.
    if (!value.startsWith("cid:"))
        return false;
    try {
        parseContainerId(value);
        return true;
    }
    catch {
        return false;
    }
}
export function idsEqual(left, right) {
    return left.peer === right.peer && left.counter === right.counter;
}
export function containerIdsEqual(left, right) {
    // Structural comparison: stringifying both ids would allocate two throwaway
    // strings per call on the B4 edit hot path.
    if (left.kind !== right.kind ||
        !codecContainerTypesEqual(left.containerType, right.containerType)) {
        return false;
    }
    return left.kind === "root" && right.kind === "root"
        ? left.name === right.name
        : left.kind === "normal" &&
            right.kind === "normal" &&
            left.peer === right.peer &&
            left.counter === right.counter;
}
function codecContainerTypesEqual(left, right) {
    return typeof left === "string" || typeof right === "string"
        ? left === right
        : left.value === right.value;
}
function assertCounter(counter) {
    if (!Number.isSafeInteger(counter) || counter < 0 || counter > 2147483647) {
        throw new RangeError(`counter is out of range: ${counter}`);
    }
}
