export function redactJsonUpdates(input, versionRange) {
    const parsed = typeof input === "string" ? JSON.parse(input) : input;
    const schema = cloneJsonUpdateValue(parsed);
    for (const change of schema.changes) {
        const peerToken = change.id.slice(change.id.indexOf("@") + 1);
        const peer = schema.peers === null ? peerToken : (schema.peers[Number(peerToken)] ?? peerToken);
        const range = versionRange[peer];
        if (range === undefined)
            continue;
        for (const operation of change.ops)
            redactOperation(operation, range);
    }
    return schema;
}
function redactOperation(operation, range) {
    const content = operation.content;
    if (content.type === "insert" && typeof content.text === "string") {
        const characters = Array.from(content.text);
        content.text = characters
            .map((character, index) => inRange(operation.counter + index, range) ? "�" : character)
            .join("");
        return;
    }
    if (content.type === "insert" && Array.isArray(content.value)) {
        content.value = content.value.map((value, index) => inRange(operation.counter + index, range) ? redactValue(value) : value);
        return;
    }
    if (!inRange(operation.counter, range))
        return;
    if (content.type === "insert" && typeof content.key === "string") {
        content.value = redactValue(content.value);
    }
    else if (content.type === "set") {
        content.value = redactValue(content.value);
    }
    else if (content.type === "mark") {
        content.style_value = null;
    }
}
function redactValue(value) {
    return typeof value === "string" && value.startsWith("🦜:")
        ? value
        : null;
}
function inRange(counter, range) {
    return counter >= range[0] && counter < range[1];
}
function cloneJsonUpdateValue(value) {
    if (value instanceof Uint8Array)
        return value.slice();
    if (Array.isArray(value))
        return value.map(cloneJsonUpdateValue);
    if (typeof value === "object" && value !== null) {
        return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, cloneJsonUpdateValue(item)]));
    }
    return value;
}
export function jsonUpdatePeer(schema, compressedPeer) {
    return (schema.peers?.[Number(compressedPeer)] ?? compressedPeer);
}
