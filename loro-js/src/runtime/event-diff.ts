import type { Delta, ListDiff, TextDiff, Value } from "./types";

type Attributes = Readonly<Record<string, Value>>;

interface OriginalPiece {
  readonly kind: "original";
  readonly start: number;
  readonly length: number;
  readonly attributes: Attributes | undefined;
}

interface TextPiece {
  readonly kind: "text";
  readonly value: string;
  readonly attributes: Attributes | undefined;
}

interface ListPiece {
  readonly kind: "list";
  readonly value: readonly unknown[];
}

type Piece = OriginalPiece | TextPiece | ListPiece;

interface PieceNode {
  piece: Piece;
  readonly priority: number;
  left: PieceNode | undefined;
  right: PieceNode | undefined;
  length: number;
}

export class SequenceEventDiff {
  readonly #kind: "text" | "list";
  readonly #originalLength: number;
  #root: PieceNode | undefined;
  #randomState = 0x7f_4a_7c_15;

  constructor(kind: "text" | "list", originalLength: number) {
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

  get length(): number {
    return nodeLength(this.#root);
  }

  insertText(position: number, value: string, attributes?: Attributes): void {
    if (this.#kind !== "text") throw new Error("cannot insert text into a list diff");
    if (value.length === 0) return;
    const [left, right] = this.#splitAt(position);
    const inserted = this.#newNode({
      kind: "text",
      value,
      attributes:
        attributes !== undefined && hasAttributes(attributes) ? attributes : undefined,
    });
    this.#root = mergeNodes(mergeNodes(left, inserted), right);
  }

  insertList(position: number, value: readonly unknown[]): void {
    if (this.#kind !== "list") throw new Error("cannot insert a list into a text diff");
    if (value.length === 0) return;
    const [left, right] = this.#splitAt(position);
    const inserted = this.#newNode({ kind: "list", value: [...value] });
    this.#root = mergeNodes(mergeNodes(left, inserted), right);
  }

  delete(position: number, length: number): void {
    if (length === 0) return;
    const [left, rest] = this.#splitAt(position);
    this.#root = rest;
    const [, right] = this.#splitAt(length);
    this.#root = mergeNodes(left, right);
  }

  formatText(position: number, length: number, key: string, value: Value): void {
    if (this.#kind !== "text") throw new Error("cannot format a list diff");
    if (length === 0) return;
    const [left, rest] = this.#splitAt(position);
    this.#root = rest;
    const [selected, right] = this.#splitAt(length);
    visitNodes(selected, (node) => {
      const piece = node.piece;
      if (piece.kind === "list") throw new Error("text diff contains a list piece");
      const attributes: Attributes = { ...piece.attributes, [key]: value };
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

  toDiff(): TextDiff | ListDiff {
    return this.#kind === "text"
      ? { type: "text", diff: this.#textDelta() }
      : { type: "list", diff: this.#listDelta() };
  }

  #textDelta(): Delta<string>[] {
    const output: Delta<string>[] = [];
    let originalCursor = 0;
    let inserted: TextPiece[] = [];
    const flushInserted = (): void => {
      let values: string[] = [];
      let attributes: Attributes | undefined;
      const flushGroup = (): void => {
        if (values.length === 0) return;
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
      if (piece.kind === "list") throw new Error("text diff contains a list piece");
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

  #listDelta(): Delta<unknown[]>[] {
    const output: Delta<unknown[]>[] = [];
    let originalCursor = 0;
    let inserted: unknown[] = [];
    const flushInserted = (): void => {
      if (inserted.length > 0) appendListDelta(output, { insert: inserted });
      inserted = [];
    };
    for (const piece of pieces(this.#root)) {
      if (piece.kind === "list") {
        inserted.push(...piece.value);
        continue;
      }
      if (piece.kind === "text") throw new Error("list diff contains a text piece");
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

  #splitAt(position: number): [PieceNode | undefined, PieceNode | undefined] {
    if (!Number.isSafeInteger(position) || position < 0 || position > this.length) {
      throw new RangeError(`event diff position ${position} is out of range`);
    }
    const result = splitNode(this.#root, position, (piece) => this.#newNode(piece));
    this.#root = undefined;
    return result;
  }

  #newNode(piece: Piece): PieceNode {
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

function pieceLength(piece: Piece): number {
  if (piece.kind === "original") return piece.length;
  return piece.value.length;
}

function splitPiece(piece: Piece, position: number): [Piece, Piece] {
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
      {
        kind: "text",
        value: piece.value.slice(0, position),
        attributes: piece.attributes,
      },
      { kind: "text", value: piece.value.slice(position), attributes: piece.attributes },
    ];
  }
  return [
    { kind: "list", value: piece.value.slice(0, position) },
    { kind: "list", value: piece.value.slice(position) },
  ];
}

function nodeLength(node: PieceNode | undefined): number {
  return node?.length ?? 0;
}

function recomputeNode(node: PieceNode): void {
  node.length = nodeLength(node.left) + pieceLength(node.piece) + nodeLength(node.right);
}

function mergeNodes(
  left: PieceNode | undefined,
  right: PieceNode | undefined,
): PieceNode | undefined {
  if (left === undefined) return right;
  if (right === undefined) return left;
  if (left.priority <= right.priority) {
    left.right = mergeNodes(left.right, right);
    recomputeNode(left);
    return left;
  }
  right.left = mergeNodes(left, right.left);
  recomputeNode(right);
  return right;
}

function splitNode(
  root: PieceNode | undefined,
  position: number,
  newNode: (piece: Piece) => PieceNode,
): [PieceNode | undefined, PieceNode | undefined] {
  if (root === undefined) return [undefined, undefined];
  const leftLength = nodeLength(root.left);
  const ownLength = pieceLength(root.piece);
  if (position < leftLength) {
    const [left, right] = splitNode(root.left, position, newNode);
    root.left = right;
    recomputeNode(root);
    return [left, root];
  }
  if (position > leftLength + ownLength) {
    const [left, right] = splitNode(
      root.right,
      position - leftLength - ownLength,
      newNode,
    );
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

function visitNodes(root: PieceNode | undefined, visit: (node: PieceNode) => void): void {
  const stack: PieceNode[] = [];
  let node = root;
  while (node !== undefined || stack.length > 0) {
    while (node !== undefined) {
      stack.push(node);
      node = node.left;
    }
    node = stack.pop()!;
    visit(node);
    node = node.right;
  }
}

function pieces(root: PieceNode | undefined): Piece[] {
  const output: Piece[] = [];
  visitNodes(root, (node) => output.push(node.piece));
  return output;
}

function appendTextDelta(output: Delta<string>[], operation: Delta<string>): void {
  const previous = output.at(-1);
  if (previous !== undefined && sameDeltaKind(previous, operation)) {
    if ("delete" in previous && "delete" in operation) {
      output[output.length - 1] = { delete: previous.delete + operation.delete };
      return;
    }
    if (
      "retain" in previous &&
      "retain" in operation &&
      attributesEqual(previous.attributes, operation.attributes)
    ) {
      output[output.length - 1] = {
        retain: previous.retain + operation.retain,
        ...(previous.attributes === undefined ? {} : { attributes: previous.attributes }),
      };
      return;
    }
    if (
      "insert" in previous &&
      "insert" in operation &&
      attributesEqual(previous.attributes, operation.attributes)
    ) {
      output[output.length - 1] = {
        insert: previous.insert + operation.insert,
        ...(previous.attributes === undefined ? {} : { attributes: previous.attributes }),
      };
      return;
    }
  }
  output.push(operation);
}

function appendListDelta(output: Delta<unknown[]>[], operation: Delta<unknown[]>): void {
  const previous = output.at(-1);
  if (previous !== undefined && "delete" in previous && "delete" in operation) {
    output[output.length - 1] = { delete: previous.delete + operation.delete };
  } else if (previous !== undefined && "retain" in previous && "retain" in operation) {
    output[output.length - 1] = { retain: previous.retain + operation.retain };
  } else if (previous !== undefined && "insert" in previous && "insert" in operation) {
    for (const item of operation.insert) previous.insert.push(item);
  } else {
    output.push(operation);
  }
}

function sameDeltaKind<T>(left: Delta<T>, right: Delta<T>): boolean {
  return (
    ("insert" in left && "insert" in right) ||
    ("delete" in left && "delete" in right) ||
    ("retain" in left && "retain" in right)
  );
}

function attributesEqual(
  left: Attributes | undefined,
  right: Attributes | undefined,
): boolean {
  if (left === right) return true;
  if (left === undefined || right === undefined) return false;
  let count = 0;
  for (const key in left) {
    count += 1;
    if (!Object.is(left[key], right[key])) return false;
  }
  for (const _key in right) count -= 1;
  return count === 0;
}

function hasAttributes(attributes: Attributes): boolean {
  for (const _key in attributes) return true;
  return false;
}

function trimPlainTrailingRetains<T>(output: Delta<T>[]): void {
  while (true) {
    const last = output.at(-1);
    if (last === undefined || !("retain" in last) || last.attributes !== undefined)
      return;
    output.pop();
  }
}
