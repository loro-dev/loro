interface OrderedNode<T extends object> {
  readonly value: T;
  readonly priority: number;
  left: OrderedNode<T> | undefined;
  right: OrderedNode<T> | undefined;
  parent: OrderedNode<T> | undefined;
  size: number;
}

export class OrderedIndex<T extends object> {
  #root: OrderedNode<T> | undefined;
  #last: OrderedNode<T> | undefined;
  #randomState = 0x85_eb_ca_6b;
  readonly #compare: (left: T, right: T) => number;
  #nodes = new WeakMap<T, OrderedNode<T>>();

  constructor(compare: (left: T, right: T) => number) {
    this.#compare = compare;
  }

  get size(): number {
    return nodeSize(this.#root);
  }

  add(value: T): void {
    if (this.#nodes.has(value)) throw new Error("ordered index already contains value");
    if (this.#last !== undefined && this.#compare(this.#last.value, value) < 0) {
      const node = this.#newNode(value);
      this.#nodes.set(value, node);
      this.#root = merge(this.#root, node);
      if (this.#root !== undefined) this.#root.parent = undefined;
      this.#last = node;
      return;
    }
    const index = this.lowerBound(value);
    const previous = this.at(index);
    if (previous !== undefined && this.#compare(previous, value) === 0) {
      throw new Error("ordered index keys must be unique");
    }
    const node = this.#newNode(value);
    this.#nodes.set(value, node);
    const [left, right] = split(this.#root, index);
    this.#root = merge(merge(left, node), right);
    if (this.#root !== undefined) this.#root.parent = undefined;
    if (this.#last === undefined) this.#last = node;
  }

  delete(value: T): boolean {
    const index = this.indexOf(value);
    if (index === undefined) return false;
    const [left, selectedAndRight] = split(this.#root, index);
    const [, right] = split(selectedAndRight, 1);
    this.#root = merge(left, right);
    if (this.#root !== undefined) this.#root.parent = undefined;
    if (this.#last === this.#nodes.get(value)) {
      this.#last = this.#root === undefined ? undefined : rightmost(this.#root);
    }
    this.#nodes.delete(value);
    return true;
  }

  at(index: number): T | undefined {
    if (!Number.isSafeInteger(index) || index < 0 || index >= this.size) return undefined;
    let node = this.#root;
    let remaining = index;
    while (node !== undefined) {
      const leftSize = nodeSize(node.left);
      if (remaining < leftSize) node = node.left;
      else if (remaining === leftSize) return node.value;
      else {
        remaining -= leftSize + 1;
        node = node.right;
      }
    }
    return undefined;
  }

  indexOf(value: T): number | undefined {
    const node = this.#nodes.get(value);
    if (node === undefined) return undefined;
    let index = nodeSize(node.left);
    let current = node;
    while (current.parent !== undefined) {
      if (current === current.parent.right) {
        index += nodeSize(current.parent.left) + 1;
      }
      current = current.parent;
    }
    return current === this.#root ? index : undefined;
  }

  lowerBound(value: T): number {
    return this._lowerBoundBy((current) => this.#compare(current, value));
  }

  _lowerBoundBy(compare: (value: T) => number): number {
    let node = this.#root;
    let offset = 0;
    let answer = this.size;
    while (node !== undefined) {
      if (compare(node.value) < 0) {
        offset += nodeSize(node.left) + 1;
        node = node.right;
      } else {
        answer = offset + nodeSize(node.left);
        node = node.left;
      }
    }
    return answer;
  }

  values(): T[] {
    const output: T[] = [];
    const stack: OrderedNode<T>[] = [];
    let node = this.#root;
    while (node !== undefined || stack.length > 0) {
      while (node !== undefined) {
        stack.push(node);
        node = node.left;
      }
      node = stack.pop()!;
      output.push(node.value);
      node = node.right;
    }
    return output;
  }

  valuesRange(start: number, end: number): T[] {
    const boundedStart = Math.max(0, Math.min(start, this.size));
    const boundedEnd = Math.max(boundedStart, Math.min(end, this.size));
    const output: T[] = [];
    collectRange(this.#root, boundedStart, boundedEnd, output);
    return output;
  }

  forEachFrom(lower: T, visit: (value: T) => boolean | void): void {
    const stack: OrderedNode<T>[] = [];
    let node = this.#root;
    while (node !== undefined) {
      if (this.#compare(node.value, lower) < 0) {
        node = node.right;
      } else {
        stack.push(node);
        node = node.left;
      }
    }
    while (stack.length > 0) {
      node = stack.pop()!;
      if (visit(node.value) === false) return;
      node = node.right;
      while (node !== undefined) {
        stack.push(node);
        node = node.left;
      }
    }
  }

  clear(): void {
    this.#root = undefined;
    this.#last = undefined;
    this.#nodes = new WeakMap();
  }

  #newNode(value: T): OrderedNode<T> {
    this.#randomState ^= this.#randomState << 13;
    this.#randomState ^= this.#randomState >>> 17;
    this.#randomState ^= this.#randomState << 5;
    return {
      value,
      priority: this.#randomState >>> 0,
      left: undefined,
      right: undefined,
      parent: undefined,
      size: 1,
    };
  }
}

function nodeSize<T extends object>(node: OrderedNode<T> | undefined): number {
  return node?.size ?? 0;
}

function rightmost<T extends object>(node: OrderedNode<T>): OrderedNode<T> {
  while (node.right !== undefined) node = node.right;
  return node;
}

function collectRange<T extends object>(
  node: OrderedNode<T> | undefined,
  start: number,
  end: number,
  output: T[],
): void {
  if (node === undefined || start >= end) return;
  const leftSize = nodeSize(node.left);
  if (start < leftSize) {
    collectRange(node.left, start, Math.min(end, leftSize), output);
  }
  if (start <= leftSize && leftSize < end) output.push(node.value);
  if (end > leftSize + 1) {
    collectRange(
      node.right,
      Math.max(0, start - leftSize - 1),
      end - leftSize - 1,
      output,
    );
  }
}

function recompute<T extends object>(node: OrderedNode<T>): void {
  node.size = nodeSize(node.left) + 1 + nodeSize(node.right);
  if (node.left !== undefined) node.left.parent = node;
  if (node.right !== undefined) node.right.parent = node;
}

function merge<T extends object>(
  left: OrderedNode<T> | undefined,
  right: OrderedNode<T> | undefined,
): OrderedNode<T> | undefined {
  if (left === undefined) {
    if (right !== undefined) right.parent = undefined;
    return right;
  }
  if (right === undefined) {
    left.parent = undefined;
    return left;
  }
  if (left.priority <= right.priority) {
    left.right = merge(left.right, right);
    recompute(left);
    left.parent = undefined;
    return left;
  }
  right.left = merge(left, right.left);
  recompute(right);
  right.parent = undefined;
  return right;
}

function split<T extends object>(
  root: OrderedNode<T> | undefined,
  count: number,
): [OrderedNode<T> | undefined, OrderedNode<T> | undefined] {
  if (root === undefined) return [undefined, undefined];
  if (nodeSize(root.left) >= count) {
    const [left, right] = split(root.left, count);
    root.left = right;
    recompute(root);
    root.parent = undefined;
    if (left !== undefined) left.parent = undefined;
    return [left, root];
  }
  const [left, right] = split(root.right, count - nodeSize(root.left) - 1);
  root.right = left;
  recompute(root);
  root.parent = undefined;
  if (right !== undefined) right.parent = undefined;
  return [root, right];
}
