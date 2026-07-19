export class LoroDecodeError extends Error {
    offset;
    constructor(message, offset) {
        super(offset === undefined ? message : `${message} at byte ${offset}`);
        this.name = "LoroDecodeError";
        this.offset = offset;
    }
}
export class LoroEncodeError extends Error {
    constructor(message) {
        super(message);
        this.name = "LoroEncodeError";
    }
}
export function decodeAssert(condition, message, offset) {
    if (!condition) {
        throw new LoroDecodeError(message, offset);
    }
}
export function encodeAssert(condition, message) {
    if (!condition) {
        throw new LoroEncodeError(message);
    }
}
