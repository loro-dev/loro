export class SequenceEventDiff {
    #kind;
    #originalLength;
    #root;
    #randomState = 2135587861;
    constructor(kind, originalLength) {
        this.#kind = kind;
        this.#originalLength = originalLength;
        this.#root =
            originalLength === 0
                ? undefined
                : this.#newNode({
                    kind: "original",
                    start: 0,
                    length: originalLength,
                    attributes: undefined,
                });
    }
    get length() {
        return nodeLength(this.#root);
    }
    insertText(position, value, attributes) {
        if (this.#kind !== "text")
            throw new Error("cannot insert text into a list diff");
        if (value.length === 0)
            return;
        const [left, right] = this.#splitAt(position);
        const inserted = this.#newNode({
            kind: "text",
            value,
            attributes: attributes !== undefined && hasAttributes(attributes) ? attributes : undefined,
        });
        this.#root = mergeNodes(mergeNodes(left, inserted), right);
    }
    insertList(position, value) {
        if (this.#kind !== "list")
            throw new Error("cannot insert a list into a text diff");
        if (value.length === 0)
            return;
        const [left, right] = this.#splitAt(position);
        const inserted = this.#newNode({ kind: "list", value: [...value] });
        this.#root = mergeNodes(mergeNodes(left, inserted), right);
    }
    delete(position, length) {
        if (length === 0)
            return;
        const [left, rest] = this.#splitAt(position);
        this.#root = rest;
        const [, right] = this.#splitAt(length);
        this.#root = mergeNodes(left, right);
    }
    formatText(position, length, key, value) {
        if (this.#kind !== "text")
            throw new Error("cannot format a list diff");
        if (length === 0)
            return;
        const [left, rest] = this.#splitAt(position);
        this.#root = rest;
        const [selected, right] = this.#splitAt(length);
        visitNodes(selected, (node) => {
            const piece = node.piece;
            if (piece.kind === "list")
                throw new Error("text diff contains a list piece");
            const attributes = { ...piece.attributes, [key]: value };
            node.piece =
                piece.kind === "original"
                    ? {
                        kind: "original",
                        start: piece.start,
                        length: piece.length,
                        attributes,
                    }
                    : { kind: "text", value: piece.value, attributes };
        });
        this.#root = mergeNodes(mergeNodes(left, selected), right);
    }
    toDiff() {
        return this.#kind === "text"
            ? { type: "text", diff: this.#textDelta() }
            : { type: "list", diff: this.#listDelta() };
    }
    #textDelta() {
        const output = [];
        let originalCursor = 0;
        let inserted = [];
        const flushInserted = () => {
            let values = [];
            let attributes;
            const flushGroup = () => {
                if (values.length === 0)
                    return;
                appendTextDelta(output, {
                    insert: values.join(""),
                    ...(attributes === undefined ? {} : { attributes }),
                });
                values = [];
            };
            for (const piece of inserted) {
                if (values.length > 0 && !attributesEqual(attributes, piece.attributes)) {
                    flushGroup();
                }
                attributes = piece.attributes;
                values.push(piece.value);
            }
            flushGroup();
            inserted = [];
        };
        for (const piece of pieces(this.#root)) {
            if (piece.kind === "text") {
                inserted.push(piece);
                continue;
            }
            if (piece.kind === "list")
                throw new Error("text diff contains a list piece");
            if (piece.start < originalCursor) {
                throw new Error("text diff original pieces are out of order");
            }
            if (piece.start > originalCursor) {
                appendTextDelta(output, { delete: piece.start - originalCursor });
            }
            flushInserted();
            appendTextDelta(output, {
                retain: piece.length,
                ...(piece.attributes === undefined ? {} : { attributes: piece.attributes }),
            });
            originalCursor = piece.start + piece.length;
        }
        if (originalCursor < this.#originalLength) {
            appendTextDelta(output, { delete: this.#originalLength - originalCursor });
        }
        flushInserted();
        trimPlainTrailingRetains(output);
        return output;
    }
    #listDelta() {
        const output = [];
        let originalCursor = 0;
        let inserted = [];
        const flushInserted = () => {
            if (inserted.length > 0)
                appendListDelta(output, { insert: inserted });
            inserted = [];
        };
        for (const piece of pieces(this.#root)) {
            if (piece.kind === "list") {
                inserted.push(...piece.value);
                continue;
            }
            if (piece.kind === "text")
                throw new Error("list diff contains a text piece");
            if (piece.start < originalCursor) {
                throw new Error("list diff original pieces are out of order");
            }
            if (piece.start > originalCursor) {
                appendListDelta(output, { delete: piece.start - originalCursor });
            }
            flushInserted();
            appendListDelta(output, { retain: piece.length });
            originalCursor = piece.start + piece.length;
        }
        if (originalCursor < this.#originalLength) {
            appendListDelta(output, { delete: this.#originalLength - originalCursor });
        }
        flushInserted();
        trimPlainTrailingRetains(output);
        return output;
    }
    #splitAt(position) {
        if (!Number.isSafeInteger(position) || position < 0 || position > this.length) {
            throw new RangeError(`event diff position ${position} is out of range`);
        }
        const result = splitNode(this.#root, position, (piece) => this.#newNode(piece));
        this.#root = undefined;
        return result;
    }
    #newNode(piece) {
        this.#randomState ^= this.#randomState << 13;
        this.#randomState ^= this.#randomState >>> 17;
        this.#randomState ^= this.#randomState << 5;
        return {
            piece,
            priority: this.#randomState >>> 0,
            left: undefined,
            right: undefined,
            length: pieceLength(piece),
        };
    }
}
function pieceLength(piece) {
    if (piece.kind === "original")
        return piece.length;
    return piece.value.length;
}
function splitPiece(piece, position) {
    if (piece.kind === "original") {
        return [
            {
                kind: "original",
                start: piece.start,
                length: position,
                attributes: piece.attributes,
            },
            {
                kind: "original",
                start: piece.start + position,
                length: piece.length - position,
                attributes: piece.attributes,
            },
        ];
    }
    if (piece.kind === "text") {
        return [
            { kind: "text", value: piece.value.slice(0, position), attributes: piece.attributes },
            { kind: "text", value: piece.value.slice(position), attributes: piece.attributes },
        ];
    }
    return [
        { kind: "list", value: piece.value.slice(0, position) },
        { kind: "list", value: piece.value.slice(position) },
    ];
}
function nodeLength(node) {
    return node?.length ?? 0;
}
function recomputeNode(node) {
    node.length = nodeLength(node.left) + pieceLength(node.piece) + nodeLength(node.right);
}
function mergeNodes(left, right) {
    if (left === undefined)
        return right;
    if (right === undefined)
        return left;
    if (left.priority <= right.priority) {
        left.right = mergeNodes(left.right, right);
        recomputeNode(left);
        return left;
    }
    right.left = mergeNodes(left, right.left);
    recomputeNode(right);
    return right;
}
function splitNode(root, position, newNode) {
    if (root === undefined)
        return [undefined, undefined];
    const leftLength = nodeLength(root.left);
    const ownLength = pieceLength(root.piece);
    if (position < leftLength) {
        const [left, right] = splitNode(root.left, position, newNode);
        root.left = right;
        recomputeNode(root);
        return [left, root];
    }
    if (position > leftLength + ownLength) {
        const [left, right] = splitNode(root.right, position - leftLength - ownLength, newNode);
        root.right = left;
        recomputeNode(root);
        return [root, right];
    }
    if (position === leftLength) {
        const left = root.left;
        root.left = undefined;
        recomputeNode(root);
        return [left, root];
    }
    if (position === leftLength + ownLength) {
        const right = root.right;
        root.right = undefined;
        recomputeNode(root);
        return [root, right];
    }
    const [leftPiece, rightPiece] = splitPiece(root.piece, position - leftLength);
    return [
        mergeNodes(root.left, newNode(leftPiece)),
        mergeNodes(newNode(rightPiece), root.right),
    ];
}
function visitNodes(root, visit) {
    const stack = [];
    let node = root;
    while (node !== undefined || stack.length > 0) {
        while (node !== undefined) {
            stack.push(node);
            node = node.left;
        }
        node = stack.pop();
        visit(node);
        node = node.right;
    }
}
function pieces(root) {
    const output = [];
    visitNodes(root, (node) => output.push(node.piece));
    return output;
}
function appendTextDelta(output, operation) {
    const previous = output.at(-1);
    if (previous !== undefined && sameDeltaKind(previous, operation)) {
        if ("delete" in previous && "delete" in operation) {
            output[output.length - 1] = { delete: previous.delete + operation.delete };
            return;
        }
        if ("retain" in previous &&
            "retain" in operation &&
            attributesEqual(previous.attributes, operation.attributes)) {
            output[output.length - 1] = {
                retain: previous.retain + operation.retain,
                ...(previous.attributes === undefined ? {} : { attributes: previous.attributes }),
            };
            return;
        }
        if ("insert" in previous &&
            "insert" in operation &&
            attributesEqual(previous.attributes, operation.attributes)) {
            output[output.length - 1] = {
                insert: previous.insert + operation.insert,
                ...(previous.attributes === undefined ? {} : { attributes: previous.attributes }),
            };
            return;
        }
    }
    output.push(operation);
}
function appendListDelta(output, operation) {
    const previous = output.at(-1);
    if (previous !== undefined && "delete" in previous && "delete" in operation) {
        output[output.length - 1] = { delete: previous.delete + operation.delete };
    }
    else if (previous !== undefined && "retain" in previous && "retain" in operation) {
        output[output.length - 1] = { retain: previous.retain + operation.retain };
    }
    else if (previous !== undefined && "insert" in previous && "insert" in operation) {
        for (const item of operation.insert)
            previous.insert.push(item);
    }
    else {
        output.push(operation);
    }
}
function sameDeltaKind(left, right) {
    return (("insert" in left && "insert" in right) ||
        ("delete" in left && "delete" in right) ||
        ("retain" in left && "retain" in right));
}
function attributesEqual(left, right) {
    if (left === right)
        return true;
    if (left === undefined || right === undefined)
        return false;
    let count = 0;
    for (const key in left) {
        count += 1;
        if (!Object.is(left[key], right[key]))
            return false;
    }
    for (const _key in right)
        count -= 1;
    return count === 0;
}
function hasAttributes(attributes) {
    for (const _key in attributes)
        return true;
    return false;
}
function trimPlainTrailingRetains(output) {
    while (true) {
        const last = output.at(-1);
        if (last === undefined || !("retain" in last) || last.attributes !== undefined)
            return;
        output.pop();
    }
}
