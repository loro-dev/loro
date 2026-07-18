import { OrderedIndex } from "./ordered-index";
import { SequenceDeletionIndex } from "./sequence-deletion-index";

export interface SequenceId {
  readonly peer: bigint;
  readonly counter: number;
}

export interface IndexedSequenceElement {
  readonly id: SequenceId;
  readonly lamport?: number;
  deletedBy?: SequenceId[] | undefined;
  deletedByPeer?: bigint | undefined;
  deletedByCounter?: number | undefined;
  deleted: boolean;
}

export interface SequenceMetrics {
  readonly utf16: number;
  readonly utf8: number;
}

export interface SequenceIdRun {
  readonly start: SequenceId;
  readonly length: number;
}

export interface SequenceMetricRange {
  readonly start: number;
  readonly end: number;
}

/**
 * A compact physical sequence span. Implementations can keep scalar fields in
 * columns and only create a T view when an API actually asks for one.
 */
export abstract class SequenceSpan<T extends IndexedSequenceElement> {
  abstract readonly length: number;
  abstract elementAt(offset: number): T;
  abstract idAt(offset: number): SequenceId;
  peerAt(offset: number): bigint {
    return this.idAt(offset).peer;
  }
  counterAt(offset: number): number {
    return this.idAt(offset).counter;
  }
  abstract lamportAt(offset: number): number | undefined;
  abstract deletedAt(offset: number): boolean;
  abstract setDeletedAt(offset: number, deleted: boolean): void;
  abstract metricsAt(offset: number): SequenceMetrics;
  abstract slice(start: number, end: number): SequenceSpan<T>;

  idsAreUnique(): boolean {
    return false;
  }

  append(_other: SequenceSpan<T>): boolean {
    return false;
  }

  deletionIdsAt(_offset: number): readonly SequenceId[] {
    return [];
  }

  retain(_offset: number, _element: T): void {}

  forEachRetained(_visit: (element: T, offset: number) => void): void {}
}

type Metric = keyof SequenceMetrics;

export interface SequenceView<T> {
  readonly length: number;
  at(index: number): T | undefined;
  range(start: number, end: number): T[];
  idRuns(start: number, end: number): SequenceIdRun[];
}

type SequenceNodeStorage<T extends IndexedSequenceElement> = T | T[] | SequenceSpan<T>;

interface SequenceNode<T extends IndexedSequenceElement> {
  element: SequenceNodeStorage<T>;
  readonly isSpan: boolean;
  ownCount: number;
  readonly locationId: number;
  readonly priority: number;
  left: SequenceNode<T> | undefined;
  right: SequenceNode<T> | undefined;
  parent: SequenceNode<T> | undefined;
  ownVisibleCount: number;
  ownVisibleUtf16: number;
  ownVisibleUtf8: number;
  ownFirstVisibleId: SequenceId | undefined;
  ownLastVisibleId: SequenceId | undefined;
  ownVisibleIdRunCount: number;
  ownFirstId: SequenceId;
  ownLastId: SequenceId;
  ownIdRunCount: number;
  ownUtf16: number;
  ownUtf8: number;
  visibleOffsets: number[] | undefined;
  visibleUtf16Prefix: number[] | undefined;
  visibleUtf8Prefix: number[] | undefined;
  allCount: number;
  visibleCount: number;
  visibleUtf16: number;
  visibleUtf8: number;
  firstVisibleId: SequenceId | undefined;
  lastVisibleId: SequenceId | undefined;
  visibleIdRunCount: number;
  firstId: SequenceId;
  lastId: SequenceId;
  idRunCount: number;
  allUtf16: number;
  allUtf8: number;
  lazyDeleted: boolean;
  lazyVisible: boolean;
}

const SEQUENCE_NODE = Symbol("sequenceNode");
const SEQUENCE_OFFSET = Symbol("sequenceOffset");

type LocatedSequenceElement<T extends IndexedSequenceElement> = T & {
  [SEQUENCE_NODE]?: SequenceNode<T>;
  [SEQUENCE_OFFSET]?: number;
};

type SequenceLocation<T extends IndexedSequenceElement> = T | number;

interface StableSequenceId extends SequenceId {
  readonly stableSequenceId: true;
}

type DeletionLocation<T extends IndexedSequenceElement> = T | StableSequenceId;

interface CachedCausalView<T> {
  readonly version: Map<bigint, number>;
  readonly view: SequenceView<T>;
}

const MAX_CACHED_CAUSAL_VIEWS = 8;
const MAX_SEQUENCE_SPAN = 32;
const SEQUENCE_LOCATION_STRIDE = 64;
const listMetrics = (): SequenceMetrics => ({ utf16: 1, utf8: 1 });

export class SequenceIndex<T extends IndexedSequenceElement> {
  #root: SequenceNode<T> | undefined;
  #tail: SequenceNode<T> | undefined;
  #structureVersion = 0;
  #randomState = 0x9e_37_79_b9;
  readonly #metrics: (element: T) => SequenceMetrics;
  readonly #locationsByPeer = new Map<bigint, CounterStore<SequenceLocation<T>>>();
  readonly #locationsByLamportPeer = new Map<bigint, Map<number, SequenceLocation<T>>>();
  readonly #nodesByLocationId: SequenceNode<T>[] = [];
  readonly #deletionsByPeer = new Map<
    bigint,
    CounterStore<DeletionLocation<T> | Set<DeletionLocation<T>>>
  >();
  readonly #scalarDeletionTargetsByPeer = new Map<
    bigint,
    PagedCounterStore<DeletionLocation<T>>
  >();
  readonly #deletionRuns = new SequenceDeletionIndex();
  readonly #maxCounterByPeer = new Map<bigint, number>();
  readonly #cachedCausalViews: CachedCausalView<T>[] = [];
  #hasLazyDeletions = false;

  constructor(metrics: (element: T) => SequenceMetrics = listMetrics) {
    this.#metrics = metrics;
  }

  get allLength(): number {
    return allCount(this.#root);
  }

  get visibleLength(): number {
    return visibleCount(this.#root);
  }

  get visibleUtf16Length(): number {
    return visibleMetric(this.#root, "utf16");
  }

  get visibleUtf8Length(): number {
    return visibleMetric(this.#root, "utf8");
  }

  get spanCount(): number {
    let count = 0;
    visitInOrder(this.#root, () => {
      count += 1;
    });
    return count;
  }

  /** Changes whenever the physical order or membership changes. */
  get structureVersion(): number {
    return this.#structureVersion;
  }

  all(): T[] {
    if (this.#hasLazyDeletions) {
      materializeAllDeletions(this.#root, this.#metrics);
      this.#hasLazyDeletions = false;
    }
    const output: T[] = [];
    visitInOrder(this.#root, (node) => appendNodeElements(node, output));
    return output;
  }

  visible(): T[] {
    const output: T[] = [];
    this.forEachVisible((element) => {
      output.push(element);
    });
    return output;
  }

  forEachVisible(visit: (element: T) => boolean | void): void {
    if (this.visibleLength === 0) return;
    const stack: SequenceNode<T>[] = [];
    let node = this.#root;
    while (node !== undefined || stack.length > 0) {
      while (node !== undefined) {
        pushNodeDeletion(node, this.#metrics);
        stack.push(node);
        node = visibleCount(node.left) > 0 ? node.left : undefined;
      }
      node = stack.pop()!;
      for (let offset = 0; offset < nodeLength(node); offset += 1) {
        const element = nodeElement(node, offset);
        if (!element.deleted && visit(element) === false) return;
      }
      node = visibleCount(node.right) > 0 ? node.right : undefined;
    }
  }

  visibleRange(start: number, end: number): T[] {
    const output: T[] = [];
    this.forEachVisibleRange(start, end, (element) => {
      output.push(element);
    });
    return output;
  }

  forEachVisibleRange(
    start: number,
    end: number,
    visit: (element: T) => boolean | void,
  ): void {
    const boundedStart = Math.max(0, Math.min(start, this.visibleLength));
    const boundedEnd = Math.max(boundedStart, Math.min(end, this.visibleLength));
    visitVisibleRange(this.#root, boundedStart, boundedEnd, this.#metrics, visit);
  }

  someVisibleRange(
    start: number,
    end: number,
    predicate: (element: T) => boolean,
  ): boolean {
    let matched = false;
    this.forEachVisibleRange(start, end, (element) => {
      if (!predicate(element)) return;
      matched = true;
      return false;
    });
    return matched;
  }

  visibleIdRuns(start: number, end: number): SequenceIdRun[] {
    const boundedStart = Math.max(0, Math.min(start, this.visibleLength));
    const boundedEnd = Math.max(boundedStart, Math.min(end, this.visibleLength));
    const runs: { start: SequenceId; length: number }[] = [];
    const append = (id: SequenceId, length = 1): void => {
      if (length === 0) return;
      const previous = runs[runs.length - 1];
      if (
        previous !== undefined &&
        previous.start.peer === id.peer &&
        previous.start.counter + previous.length === id.counter
      ) {
        previous.length += length;
      } else {
        runs.push({ start: { ...id }, length });
      }
    };
    visitVisibleIdRuns(this.#root, boundedStart, boundedEnd, this.#metrics, append);
    return runs;
  }

  visibleMetricRangesForIdRuns(
    runs: readonly SequenceIdRun[],
    metric: Metric,
  ): SequenceMetricRange[] {
    const targets = counterRangesByPeer(runs);
    const ranges: { start: number; end: number }[] = [];
    const append = (start: number, end: number): void => {
      if (start >= end) return;
      const previous = ranges.at(-1);
      if (previous?.end === start) previous.end = end;
      else ranges.push({ start, end });
    };
    visitVisibleMetricRangesForIds(this.#root, 0, metric, targets, this.#metrics, append);
    return ranges;
  }

  containsIdRuns(runs: readonly SequenceIdRun[]): boolean {
    if (runs.length === 0) return true;
    const targets = counterRangesByPeer(runs);
    const expected = [...targets.values()].reduce(
      (total, ranges) =>
        total + ranges.reduce((subtotal, range) => subtotal + range.end - range.start, 0),
      0,
    );
    return countPhysicalIdsInRanges(this.#root, targets) === expected;
  }

  atPhysical(index: number): T | undefined {
    const element = this.atPhysicalRaw(index);
    if (element !== undefined && this.#hasLazyDeletions) {
      materializeElementDeletion(element, this.#metrics);
    }
    return element;
  }

  atPhysicalRaw(index: number): T | undefined {
    if (!Number.isSafeInteger(index) || index < 0 || index >= this.allLength) {
      return undefined;
    }
    let node = this.#root;
    let remaining = index;
    while (node !== undefined) {
      const leftCount = allCount(node.left);
      if (remaining < leftCount) {
        node = node.left;
      } else if (remaining < leftCount + nodeLength(node)) {
        return nodeElement(node, remaining - leftCount);
      } else {
        remaining -= leftCount + nodeLength(node);
        node = node.right;
      }
    }
    return undefined;
  }

  forEachPhysicalRaw(visit: (element: T, index: number) => boolean | void): void {
    let index = 0;
    let stopped = false;
    visitInOrder(this.#root, (node) => {
      if (stopped) return;
      for (let offset = 0; offset < nodeLength(node); offset += 1) {
        if (visit(nodeElement(node, offset), index) === false) {
          stopped = true;
          return;
        }
        index += 1;
      }
    });
  }

  findNextIncludedPhysical(
    start: number,
    version: ReadonlyMap<bigint, number>,
  ): { readonly element: T; readonly index: number } | undefined {
    if (!Number.isSafeInteger(start) || start < 0 || start >= this.allLength) {
      return undefined;
    }
    return findNextIncludedPhysical(this.#root, 0, start, version);
  }

  atVisible(index: number): T | undefined {
    if (!Number.isSafeInteger(index) || index < 0 || index >= this.visibleLength) {
      return undefined;
    }
    let node = this.#root;
    let remaining = index;
    while (node !== undefined) {
      pushNodeDeletion(node, this.#metrics);
      const leftCount = visibleCount(node.left);
      if (remaining < leftCount) {
        node = node.left;
        continue;
      }
      remaining -= leftCount;
      if (remaining < ownVisibleCount(node)) {
        return nodeElement(node, physicalOffsetAtVisibleIndex(node, remaining));
      }
      remaining -= ownVisibleCount(node);
      node = node.right;
    }
    return undefined;
  }

  visibleInsertionContext(position: number): {
    readonly current: true;
    readonly left: T | undefined;
    readonly startIndex: number;
    readonly right: T | undefined;
  } {
    if (
      !Number.isSafeInteger(position) ||
      position < 0 ||
      position > this.visibleLength
    ) {
      throw new RangeError(`visible sequence position ${position} is out of range`);
    }
    if (position === 0) {
      return {
        current: true,
        left: undefined,
        startIndex: 0,
        right: firstPhysicalElement(this.#root),
      };
    }

    let node = this.#root;
    let remaining = position - 1;
    let physicalBase = 0;
    while (node !== undefined) {
      pushNodeDeletion(node, this.#metrics);
      const leftPhysicalCount = allCount(node.left);
      const leftVisibleCount = visibleCount(node.left);
      if (remaining < leftVisibleCount) {
        node = node.left;
        continue;
      }
      remaining -= leftVisibleCount;
      physicalBase += leftPhysicalCount;
      if (remaining < ownVisibleCount(node)) {
        const offset = physicalOffsetAtVisibleIndex(node, remaining);
        const left = nodeElement(node, offset);
        return {
          current: true,
          left,
          startIndex: physicalBase + offset + 1,
          right: nextPhysicalElement(node, offset),
        };
      }
      remaining -= ownVisibleCount(node);
      physicalBase += nodeLength(node);
      node = node.right;
    }
    throw new Error("visible sequence insertion position is missing");
  }

  findById(id: SequenceId): T | undefined {
    const element = this.findByIdRaw(id);
    if (element !== undefined && this.#hasLazyDeletions) {
      materializeElementDeletion(element, this.#metrics);
    }
    return element;
  }

  findByIdRaw(id: SequenceId): T | undefined {
    const location = this.#locationsByPeer.get(id.peer)?.get(id.counter);
    return location === undefined
      ? undefined
      : typeof location === "number"
        ? this.#elementAtLocation(location)
        : location;
  }

  findByLamport(peer: bigint, lamport: number): T | undefined {
    let lamports = this.#locationsByLamportPeer.get(peer);
    if (lamports === undefined) {
      const locations = this.#locationsByPeer.get(peer);
      if (locations === undefined) return undefined;
      const indexedLamports = new Map<number, SequenceLocation<T>>();
      locations.forEach(0, Number.MAX_SAFE_INTEGER, (location) => {
        const lamport =
          typeof location === "number"
            ? nodeLamport(
                this.#nodeAtLocation(location),
                location % SEQUENCE_LOCATION_STRIDE,
              )
            : location.lamport;
        if (lamport !== undefined) {
          indexedLamports.set(lamport, location);
        }
      });
      lamports = indexedLamports;
      this.#locationsByLamportPeer.set(peer, lamports);
    }
    const location = lamports.get(lamport);
    const element =
      location === undefined
        ? undefined
        : typeof location === "number"
          ? this.#elementAtLocation(location)
          : location;
    if (element !== undefined && this.#hasLazyDeletions) {
      materializeElementDeletion(element, this.#metrics);
    }
    return element;
  }

  physicalIndexOf(element: T): number | undefined {
    const node = elementNode(element);
    const offset = elementOffset(element);
    if (node === undefined || offset === undefined) return undefined;
    let index = allCount(node.left) + offset;
    let current = node;
    while (current.parent !== undefined) {
      if (current === current.parent.right) {
        index += allCount(current.parent.left) + nodeLength(current.parent);
      }
      current = current.parent;
    }
    return current === this.#root ? index : undefined;
  }

  visibleIndexOf(element: T): number | undefined {
    if (this.#hasLazyDeletions) materializeElementDeletion(element, this.#metrics);
    const node = elementNode(element);
    const offset = elementOffset(element);
    if (node === undefined || offset === undefined) return undefined;
    let index = visibleCount(node.left) + visibleElementsBefore(node, offset);
    let current = node;
    while (current.parent !== undefined) {
      if (current === current.parent.right) {
        index += visibleCount(current.parent.left) + ownVisibleCount(current.parent);
      }
      current = current.parent;
    }
    return current === this.#root ? index : undefined;
  }

  visibleMetricOffsetOf(element: T, metric: Metric): number | undefined {
    if (this.#hasLazyDeletions) materializeElementDeletion(element, this.#metrics);
    const node = elementNode(element);
    const elementPhysicalOffset = elementOffset(element);
    if (node === undefined || elementPhysicalOffset === undefined) return undefined;
    let offset =
      visibleMetric(node.left, metric) +
      visibleMetricBefore(node, elementPhysicalOffset, metric, this.#metrics);
    let current = node;
    while (current.parent !== undefined) {
      if (current === current.parent.right) {
        offset +=
          visibleMetric(current.parent.left, metric) +
          ownVisibleMetric(current.parent, metric, this.#metrics);
      }
      current = current.parent;
    }
    return current === this.#root ? offset : undefined;
  }

  metricOffsetAtVisibleIndex(index: number, metric: Metric): number | undefined {
    if (!Number.isSafeInteger(index) || index < 0 || index > this.visibleLength) {
      return undefined;
    }
    if (index === this.visibleLength) return visibleMetric(this.#root, metric);
    let node = this.#root;
    let remaining = index;
    let offset = 0;
    while (node !== undefined) {
      pushNodeDeletion(node, this.#metrics);
      const leftCount = visibleCount(node.left);
      if (remaining < leftCount) {
        node = node.left;
        continue;
      }
      remaining -= leftCount;
      offset += visibleMetric(node.left, metric);
      if (remaining < ownVisibleCount(node)) {
        const ownOffset = physicalOffsetAtVisibleIndex(node, remaining);
        return offset + visibleMetricBefore(node, ownOffset, metric, this.#metrics);
      }
      remaining -= ownVisibleCount(node);
      offset += ownVisibleMetric(node, metric, this.#metrics);
      node = node.right;
    }
    return undefined;
  }

  visibleIndexAtMetricOffset(offset: number, metric: Metric): number | undefined {
    const total = visibleMetric(this.#root, metric);
    if (!Number.isSafeInteger(offset) || offset < 0 || offset > total) return undefined;
    if (offset === total) return this.visibleLength;
    let node = this.#root;
    let remaining = offset;
    let index = 0;
    while (node !== undefined) {
      pushNodeDeletion(node, this.#metrics);
      const leftMetric = visibleMetric(node.left, metric);
      if (remaining < leftMetric) {
        node = node.left;
        continue;
      }
      remaining -= leftMetric;
      index += visibleCount(node.left);
      const ownMetric = ownVisibleMetric(node, metric, this.#metrics);
      if (remaining <= ownMetric) {
        const ownOffset = physicalOffsetAtMetricOffset(node, remaining, metric);
        return ownOffset === undefined
          ? undefined
          : index + visibleElementsBefore(node, ownOffset);
      }
      remaining -= ownMetric;
      index += ownVisibleCount(node);
      node = node.right;
    }
    return undefined;
  }

  insertAtVisible(position: number, elements: readonly T[]): void {
    if (position < 0 || position > this.visibleLength) {
      throw new RangeError(`visible sequence position ${position} is out of range`);
    }
    const physical =
      position === this.visibleLength
        ? this.allLength
        : this.physicalIndexOf(this.atVisible(position)!)!;
    this.insertAtPhysical(physical, elements);
  }

  insertSpanAtVisible(position: number, span: SequenceSpan<T>): void {
    if (position < 0 || position > this.visibleLength) {
      throw new RangeError(`visible sequence position ${position} is out of range`);
    }
    const physical =
      position === this.visibleLength
        ? this.allLength
        : this.physicalIndexOf(this.atVisible(position)!)!;
    this.insertSpanAtPhysical(physical, span);
  }

  insertAtPhysical(position: number, elements: readonly T[]): void {
    if (elements.length === 0) return;
    if (position < 0 || position > this.allLength) {
      throw new RangeError(`physical sequence position ${position} is out of range`);
    }
    if (elements.length === 1) {
      const id = elements[0]!.id;
      if (this.#locationsByPeer.get(id.peer)?.has(id.counter) === true) {
        throw new Error(`duplicate sequence id ${id.counter}@${id.peer.toString()}`);
      }
    } else if (elements.length <= MAX_SEQUENCE_SPAN) {
      for (let index = 0; index < elements.length; index += 1) {
        const id = elements[index]!.id;
        if (this.#locationsByPeer.get(id.peer)?.has(id.counter) === true) {
          throw new Error(`duplicate sequence id ${id.counter}@${id.peer.toString()}`);
        }
        for (let previous = 0; previous < index; previous += 1) {
          const other = elements[previous]!.id;
          if (other.peer === id.peer && other.counter === id.counter) {
            throw new Error(`duplicate sequence id ${id.counter}@${id.peer.toString()}`);
          }
        }
      }
    } else {
      const ids = new Set<string>();
      for (const element of elements) {
        const key = `${element.id.peer}:${element.id.counter}`;
        if (
          ids.has(key) ||
          this.#locationsByPeer.get(element.id.peer)?.has(element.id.counter) === true
        ) {
          throw new Error(
            `duplicate sequence id ${element.id.counter}@${element.id.peer.toString()}`,
          );
        }
        ids.add(key);
      }
    }
    this.#invalidateCausalView();
    if (this.#appendToPreviousNode(position, elements)) {
      this.#structureVersion += 1;
      return;
    }
    let insertedRoot: SequenceNode<T> | undefined;
    if (elements.length <= MAX_SEQUENCE_SPAN) {
      insertedRoot = this.#newNode(elements, true);
    } else {
      for (let start = 0; start < elements.length; start += MAX_SEQUENCE_SPAN) {
        insertedRoot = merge(
          insertedRoot,
          this.#newNode(elements.slice(start, start + MAX_SEQUENCE_SPAN), true),
          this.#metrics,
        );
      }
    }
    const appendAtTail = position === this.allLength;
    const previousTailElement =
      appendAtTail || this.#tail === undefined
        ? undefined
        : nodeElement(this.#tail, nodeLength(this.#tail) - 1);
    if (appendAtTail) {
      this.#root = this.#appendAtTail(this.#root, insertedRoot);
    } else if (
      insertedRoot !== undefined &&
      insertedRoot.left === undefined &&
      insertedRoot.right === undefined
    ) {
      this.#root = this.#insertSingleNode(this.#root, position, insertedRoot);
    } else {
      const [left, right] = this.#split(this.#root, position);
      this.#root = merge(merge(left, insertedRoot, this.#metrics), right, this.#metrics);
    }
    if (this.#root !== undefined) this.#root.parent = undefined;
    if (previousTailElement !== undefined) {
      this.#tail = elementNode(previousTailElement)!;
    }
    this.#structureVersion += 1;
  }

  insertSpanAtPhysical(position: number, span: SequenceSpan<T>): void {
    if (span.length === 0) return;
    if (position < 0 || position > this.allLength) {
      throw new RangeError(`physical sequence position ${position} is out of range`);
    }
    if (span.length === 1) {
      const peer = span.peerAt(0);
      const counter = span.counterAt(0);
      if (this.#locationsByPeer.get(peer)?.has(counter) === true) {
        throw new Error(`duplicate sequence id ${counter}@${peer.toString()}`);
      }
    } else if (span.idsAreUnique()) {
      for (let offset = 0; offset < span.length; offset += 1) {
        const peer = span.peerAt(offset);
        const counter = span.counterAt(offset);
        if (this.#locationsByPeer.get(peer)?.has(counter) === true) {
          throw new Error(`duplicate sequence id ${counter}@${peer.toString()}`);
        }
      }
    } else if (span.length <= MAX_SEQUENCE_SPAN) {
      for (let offset = 0; offset < span.length; offset += 1) {
        const peer = span.peerAt(offset);
        const counter = span.counterAt(offset);
        if (this.#locationsByPeer.get(peer)?.has(counter) === true) {
          throw new Error(`duplicate sequence id ${counter}@${peer.toString()}`);
        }
        for (let previous = 0; previous < offset; previous += 1) {
          if (span.peerAt(previous) === peer && span.counterAt(previous) === counter) {
            throw new Error(`duplicate sequence id ${counter}@${peer.toString()}`);
          }
        }
      }
    } else {
      const ids = new Set<string>();
      for (let offset = 0; offset < span.length; offset += 1) {
        const peer = span.peerAt(offset);
        const counter = span.counterAt(offset);
        const key = `${peer}:${counter}`;
        if (ids.has(key) || this.#locationsByPeer.get(peer)?.has(counter) === true) {
          throw new Error(`duplicate sequence id ${counter}@${peer.toString()}`);
        }
        ids.add(key);
      }
    }
    this.#invalidateCausalView();
    if (this.#appendSpanToPreviousNode(position, span)) {
      this.#structureVersion += 1;
      return;
    }
    let insertedRoot: SequenceNode<T> | undefined;
    if (span.length <= MAX_SEQUENCE_SPAN) {
      insertedRoot = this.#newNode(span, true);
    } else {
      for (let start = 0; start < span.length; start += MAX_SEQUENCE_SPAN) {
        insertedRoot = merge(
          insertedRoot,
          this.#newNode(
            span.slice(start, Math.min(span.length, start + MAX_SEQUENCE_SPAN)),
            true,
          ),
          this.#metrics,
        );
      }
    }
    const appendAtTail = position === this.allLength;
    const previousTailElement =
      appendAtTail || this.#tail === undefined
        ? undefined
        : nodeElement(this.#tail, nodeLength(this.#tail) - 1);
    if (appendAtTail) {
      this.#root = this.#appendAtTail(this.#root, insertedRoot);
    } else if (
      insertedRoot !== undefined &&
      insertedRoot.left === undefined &&
      insertedRoot.right === undefined
    ) {
      this.#root = this.#insertSingleNode(this.#root, position, insertedRoot);
    } else {
      const [left, right] = this.#split(this.#root, position);
      this.#root = merge(merge(left, insertedRoot, this.#metrics), right, this.#metrics);
    }
    if (this.#root !== undefined) this.#root.parent = undefined;
    if (previousTailElement !== undefined) {
      this.#tail = elementNode(previousTailElement)!;
    }
    this.#structureVersion += 1;
  }

  setDeleted(element: T, deleted: boolean): void {
    if (this.#hasLazyDeletions) materializeElementDeletion(element, this.#metrics);
    const node = elementNode(element);
    const offset = elementOffset(element);
    if (node === undefined || offset === undefined || element.deleted === deleted) return;
    this.#invalidateCausalView();
    updateNodeElementVisibility(node, offset, element, deleted, this.#metrics);
    recomputeToRoot(node, this.#metrics, false);
  }

  deleteElement(element: T, deletedBy?: SequenceId): void {
    if (this.#hasLazyDeletions) materializeElementDeletion(element, this.#metrics);
    const node = elementNode(element);
    if (node === undefined) {
      throw new Error("cannot delete an element outside the sequence");
    }
    if (element.deleted && deletedBy === undefined) return;
    this.#invalidateCausalView();
    if (!element.deleted) {
      updateNodeElementVisibility(
        node,
        elementOffset(element)!,
        element,
        true,
        this.#metrics,
      );
      recomputeToRoot(node, this.#metrics, false);
    }
    if (deletedBy === undefined) return;
    if (element.deletedByPeer === undefined && (element.deletedBy?.length ?? 0) === 0) {
      element.deletedByPeer = deletedBy.peer;
      element.deletedByCounter = deletedBy.counter;
    } else (element.deletedBy ??= []).push(deletedBy);
    this.#recordDeletionElement(element, deletedBy);
    const previous = this.#maxCounterByPeer.get(deletedBy.peer) ?? 0;
    if (deletedBy.counter + 1 > previous) {
      this.#maxCounterByPeer.set(deletedBy.peer, deletedBy.counter + 1);
    }
  }

  recordOperationId(id: SequenceId): void {
    const previous = this.#maxCounterByPeer.get(id.peer) ?? 0;
    const next = Math.max(previous, id.counter + 1);
    if (next === previous) return;
    this.#invalidateCausalView();
    this.#maxCounterByPeer.set(id.peer, next);
  }

  addDeletion(element: T, id: SequenceId): void {
    if (elementNode(element) === undefined) {
      throw new Error("cannot delete an element outside the sequence");
    }
    this.#invalidateCausalView();
    if (element.deletedByPeer === undefined && (element.deletedBy?.length ?? 0) === 0) {
      element.deletedByPeer = id.peer;
      element.deletedByCounter = id.counter;
    } else (element.deletedBy ??= []).push(id);
    this.#recordDeletionElement(element, id);
    this.recordOperationId(id);
  }

  deleteIdSpan(startId: SequenceId, length: number, deletedBy?: SequenceId): void {
    const spanLength = Math.abs(length);
    if (spanLength === 0) return;
    if (spanLength === 1) {
      const element = this.findById(startId);
      if (element === undefined) return;
      this.deleteElement(element, deletedBy);
      return;
    }
    const targetedLimit = Math.max(64, Math.min(512, this.allLength >>> 8));
    const deleteByTree = this.#root?.idRunCount === 1 || spanLength > targetedLimit;
    const result = deleteByTree
      ? markIdRunsDeleted(
          this.#root,
          counterRangesByPeer([{ start: startId, length: spanLength }]),
          this.#metrics,
        )
      : this.#markLocatedIdSpanVisibility(startId, spanLength, true);
    if (!result.found) return;
    if (deleteByTree) this.#hasLazyDeletions = true;
    this.#invalidateCausalView();
    if (deletedBy !== undefined) {
      this.#deletionRuns.add(startId, spanLength, deletedBy, length >= 0 ? 1 : -1);
      this.recordOperationId({
        peer: deletedBy.peer,
        counter: deletedBy.counter + spanLength - 1,
      });
    }
  }

  #markLocatedIdSpanVisibility(
    startId: SequenceId,
    length: number,
    deleted: boolean,
  ): DeleteRunResult {
    const locations = this.#locationsByPeer.get(startId.peer);
    if (locations === undefined) return { found: false, changed: false };
    const elements: T[] = [];
    locations.forEach(startId.counter, startId.counter + length, (location) => {
      elements.push(
        typeof location === "number" ? this.#elementAtLocation(location) : location,
      );
    });
    if (elements.length === 0) return { found: false, changed: false };

    if (elements.length <= MAX_SEQUENCE_SPAN) {
      const materializedNodes: SequenceNode<T>[] = [];
      for (const element of elements) {
        const node = elementNode(element)!;
        if (materializedNodes.includes(node)) continue;
        materializedNodes.push(node);
        materializeElementDeletion(element, this.#metrics);
      }

      const changedNodes: SequenceNode<T>[] = [];
      for (const element of elements) {
        if (element.deleted === deleted) continue;
        element.deleted = deleted;
        const node = elementNode(element)!;
        if (!changedNodes.includes(node)) changedNodes.push(node);
      }
      if (changedNodes.length === 0) return { found: true, changed: false };
      for (const node of changedNodes) recomputeOwn(node, this.#metrics);
      for (const node of changedNodes) recomputeToRoot(node, this.#metrics, false);
      return { found: true, changed: true };
    }

    const materializedNodes = new Set<SequenceNode<T>>();
    for (const element of elements) {
      const node = elementNode(element)!;
      if (materializedNodes.has(node)) continue;
      materializedNodes.add(node);
      materializeElementDeletion(element, this.#metrics);
    }

    const changedNodes = new Set<SequenceNode<T>>();
    for (const element of elements) {
      if (element.deleted === deleted) continue;
      element.deleted = deleted;
      changedNodes.add(elementNode(element)!);
    }
    if (changedNodes.size === 0) return { found: true, changed: false };
    const affected = new Set<SequenceNode<T>>();
    for (const node of changedNodes) {
      recomputeOwn(node, this.#metrics);
      let current: SequenceNode<T> | undefined = node;
      while (current !== undefined) {
        affected.add(current);
        current = current.parent;
      }
    }
    recomputeAffected(this.#root, affected, this.#metrics);
    return { found: true, changed: true };
  }

  setIdRunsDeleted(runs: readonly SequenceIdRun[]): void {
    if (runs.length === 0) return;
    const run = runs.length === 1 ? runs[0]! : undefined;
    const targetedLimit = Math.max(64, Math.min(512, this.allLength >>> 8));
    const updateByTree =
      run === undefined || this.#root?.idRunCount === 1 || run.length > targetedLimit;
    const result = updateByTree
      ? markIdRunsDeleted(this.#root, counterRangesByPeer(runs), this.#metrics)
      : this.#markLocatedIdSpanVisibility(run.start, run.length, true);
    if (!result.changed) return;
    if (updateByTree) this.#hasLazyDeletions = true;
    this.#invalidateCausalView();
  }

  setIdRunsVisible(runs: readonly SequenceIdRun[]): void {
    if (runs.length === 0) return;
    const run = runs.length === 1 ? runs[0]! : undefined;
    const targetedLimit = Math.max(64, Math.min(512, this.allLength >>> 8));
    const updateByTree =
      run === undefined || this.#root?.idRunCount === 1 || run.length > targetedLimit;
    const result = updateByTree
      ? markIdRunsVisible(this.#root, counterRangesByPeer(runs), this.#metrics)
      : this.#markLocatedIdSpanVisibility(run.start, run.length, false);
    if (!result.changed) return;
    if (updateByTree) this.#hasLazyDeletions = true;
    this.#invalidateCausalView();
  }

  canShowIdRunsAt(
    runs: readonly SequenceIdRun[],
    version: { get(peer: bigint): number | undefined },
  ): boolean {
    for (const run of runs) {
      if (run.start.counter + run.length > (version.get(run.start.peer) ?? 0)) {
        return false;
      }
    }
    if (this.#deletionRuns.hasDeletionAt(runs, version)) return false;
    for (const run of runs) {
      if (
        this.#scalarDeletionTargetsByPeer
          .get(run.start.peer)
          ?.some(run.start.counter, run.start.counter + run.length, (location) =>
            someSequenceDeletion(
              this.#elementAtDeletionLocation(location),
              (id) => id.counter < (version.get(id.peer) ?? 0),
            ),
          )
      ) {
        return false;
      }
    }
    return true;
  }

  elementsDeletedBy(peer: bigint, start: number, end: number): T[] {
    const selected = new Set<DeletionLocation<T>>();
    for (const run of this.#deletionRuns.targetRunsDeletedBy(peer, start, end)) {
      for (let offset = 0; offset < run.length; offset += 1) {
        const location = this.#locationsByPeer
          .get(run.start.peer)
          ?.get(run.start.counter + offset);
        if (location !== undefined) {
          selected.add(
            typeof location === "number"
              ? {
                  stableSequenceId: true,
                  peer: run.start.peer,
                  counter: run.start.counter + offset,
                }
              : location,
          );
        }
      }
    }
    this.#deletionsByPeer.get(peer)?.forEach(start, end, (elementOrSet) => {
      if (elementOrSet instanceof Set) {
        for (const element of elementOrSet) selected.add(element);
      } else {
        selected.add(elementOrSet);
      }
    });
    return [...selected]
      .map((location) => {
        const element = this.#elementAtDeletionLocation(location);
        return { element, index: this.physicalIndexOf(element)! };
      })
      .sort((left, right) => left.index - right.index)
      .map(({ element }) => element);
  }

  idRunsDeletedBy(peer: bigint, start: number, end: number): SequenceIdRun[] {
    const runs = [...this.#deletionRuns.targetRunsDeletedBy(peer, start, end)];
    this.#deletionsByPeer.get(peer)?.forEach(start, end, (elementOrSet) => {
      if (elementOrSet instanceof Set) {
        for (const location of elementOrSet) {
          runs.push({ start: this.#elementAtDeletionLocation(location).id, length: 1 });
        }
      } else {
        runs.push({
          start: this.#elementAtDeletionLocation(elementOrSet).id,
          length: 1,
        });
      }
    });
    return normalizeSequenceIdRuns(runs);
  }

  someDeletion(
    element: IndexedSequenceElement,
    predicate: (id: SequenceId) => boolean,
  ): boolean {
    return (
      this.#deletionRuns.someDeletion(element.id, predicate) ||
      someSequenceDeletion(element, predicate)
    );
  }

  everyDeletion(
    element: IndexedSequenceElement,
    predicate: (id: SequenceId) => boolean,
  ): boolean {
    return (
      this.#deletionRuns.everyDeletion(element.id, predicate) &&
      everySequenceDeletion(element, predicate)
    );
  }

  moveVisible(from: number, to: number): void {
    const element = this.atVisible(from);
    if (element === undefined || from === to) return;
    this.#invalidateCausalView();
    const physical = this.physicalIndexOf(element)!;
    const [before, selectedAndAfter] = this.#split(this.#root, physical);
    const [selected, after] = this.#split(selectedAndAfter, 1);
    this.#root = merge(before, after, this.#metrics);
    const destinationElement = this.atVisible(to);
    const destination =
      destinationElement === undefined
        ? this.allLength
        : this.physicalIndexOf(destinationElement)!;
    const [left, right] = this.#split(this.#root, destination);
    this.#root = merge(merge(left, selected, this.#metrics), right, this.#metrics);
    if (this.#root !== undefined) this.#root.parent = undefined;
    this.#tail = this.#root === undefined ? undefined : rightmost(this.#root);
    this.#structureVersion += 1;
  }

  moveBefore(element: T, before: T | undefined): void {
    if (element === before) return;
    const physical = this.physicalIndexOf(element);
    if (physical === undefined) return;
    this.#invalidateCausalView();
    const [left, selectedAndRight] = this.#split(this.#root, physical);
    const [selected, right] = this.#split(selectedAndRight, 1);
    this.#root = merge(left, right, this.#metrics);
    const destination =
      before === undefined ? this.allLength : this.physicalIndexOf(before);
    if (destination === undefined) {
      this.#root = merge(this.#root, selected, this.#metrics);
    } else {
      const [beforeDestination, afterDestination] = this.#split(this.#root, destination);
      this.#root = merge(
        merge(beforeDestination, selected, this.#metrics),
        afterDestination,
        this.#metrics,
      );
    }
    if (this.#root !== undefined) this.#root.parent = undefined;
    this.#tail = this.#root === undefined ? undefined : rightmost(this.#root);
    this.#structureVersion += 1;
  }

  nextVisible(element: T): T | undefined {
    if (this.#hasLazyDeletions) materializeElementDeletion(element, this.#metrics);
    const node = elementNode(element);
    const offset = elementOffset(element);
    if (node === undefined || offset === undefined) return undefined;
    for (let index = offset + 1; index < nodeLength(node); index += 1) {
      const next = nodeElement(node, index);
      if (!next.deleted) return next;
    }
    const right = firstVisibleElement(node.right, this.#metrics);
    if (right !== undefined) return right;
    let current = node;
    while (current.parent !== undefined) {
      const parent = current.parent;
      if (current === parent.left) {
        for (let index = 0; index < nodeLength(parent); index += 1) {
          const next = nodeElement(parent, index);
          if (!next.deleted) return next;
        }
        const sibling = firstVisibleElement(parent.right, this.#metrics);
        if (sibling !== undefined) return sibling;
      }
      current = parent;
    }
    return undefined;
  }

  previousVisible(element: T): T | undefined {
    if (this.#hasLazyDeletions) materializeElementDeletion(element, this.#metrics);
    const node = elementNode(element);
    const offset = elementOffset(element);
    if (node === undefined || offset === undefined) return undefined;
    for (let index = offset - 1; index >= 0; index -= 1) {
      const previous = nodeElement(node, index);
      if (!previous.deleted) return previous;
    }
    const left = lastVisibleElement(node.left, this.#metrics);
    if (left !== undefined) return left;
    let current = node;
    while (current.parent !== undefined) {
      const parent = current.parent;
      if (current === parent.right) {
        for (let index = nodeLength(parent) - 1; index >= 0; index -= 1) {
          const previous = nodeElement(parent, index);
          if (!previous.deleted) return previous;
        }
        const sibling = lastVisibleElement(parent.left, this.#metrics);
        if (sibling !== undefined) return sibling;
      }
      current = parent;
    }
    return undefined;
  }

  isFullyIncluded(version: ReadonlyMap<bigint, number>): boolean {
    for (const [peer, counter] of this.#maxCounterByPeer) {
      if ((version.get(peer) ?? 0) < counter) return false;
    }
    return true;
  }

  causalVisible(version: ReadonlyMap<bigint, number>): T[] {
    if (this.isFullyIncluded(version)) return this.visible();
    const view = this.causalView(version);
    return view.range(0, view.length);
  }

  causalView(version: ReadonlyMap<bigint, number>): SequenceView<T> {
    if (this.isFullyIncluded(version)) {
      return {
        length: this.visibleLength,
        at: (index) => this.atVisible(index),
        range: (start, end) => this.visibleRange(start, end),
        idRuns: (start, end) => this.visibleIdRuns(start, end),
      };
    }
    const cachedIndex = this.#cachedCausalViews.findIndex((cached) =>
      versionsEqual(cached.version, version),
    );
    if (cachedIndex >= 0) {
      const cached = this.#cachedCausalViews[cachedIndex]!;
      if (cachedIndex + 1 < this.#cachedCausalViews.length) {
        this.#cachedCausalViews.splice(cachedIndex, 1);
        this.#cachedCausalViews.push(cached);
      }
      return cached.view;
    }
    const causalVersion = new Map(version);
    const included = (id: SequenceId): boolean =>
      id.counter < (causalVersion.get(id.peer) ?? 0);
    const isVisible = (element: T): boolean =>
      included(element.id) &&
      this.everyDeletion(element, (deleteId) => !included(deleteId));
    const causalCounts = new Map<SequenceNode<T>, number>();
    const causalCount = (node: SequenceNode<T> | undefined): number => {
      if (node === undefined) return 0;
      const cached = causalCounts.get(node);
      if (cached !== undefined) return cached;
      if (node.idRunCount === 1) {
        const includedEnd = causalVersion.get(node.firstId.peer) ?? 0;
        if (includedEnd <= node.firstId.counter) {
          causalCounts.set(node, 0);
          return 0;
        }
        if (
          includedEnd > node.lastId.counter &&
          this.canShowIdRunsAt(
            [{ start: node.firstId, length: node.allCount }],
            causalVersion,
          )
        ) {
          causalCounts.set(node, node.allCount);
          return node.allCount;
        }
      }
      let count = causalCount(node.left) + causalCount(node.right);
      for (let offset = 0; offset < nodeLength(node); offset += 1) {
        if (isVisible(nodeElement(node, offset))) count += 1;
      }
      causalCounts.set(node, count);
      return count;
    };
    const length = causalCount(this.#root);
    const view: SequenceView<T> = {
      length,
      at: (index) => {
        if (!Number.isSafeInteger(index) || index < 0 || index >= length) {
          return undefined;
        }
        let node = this.#root;
        let remaining = index;
        while (node !== undefined) {
          const leftCount = causalCount(node.left);
          if (remaining < leftCount) {
            node = node.left;
            continue;
          }
          remaining -= leftCount;
          for (let offset = 0; offset < nodeLength(node); offset += 1) {
            const element = nodeElement(node, offset);
            if (!isVisible(element)) continue;
            if (remaining === 0) return element;
            remaining -= 1;
          }
          node = node.right;
        }
        return undefined;
      },
      range: (start, end) => {
        const boundedStart = Math.max(0, Math.min(start, length));
        const boundedEnd = Math.max(boundedStart, Math.min(end, length));
        const output: T[] = [];
        collectCausalRange(
          this.#root,
          boundedStart,
          boundedEnd,
          output,
          causalCount,
          isVisible,
          this.#metrics,
        );
        return output;
      },
      idRuns: (start, end) => {
        const boundedStart = Math.max(0, Math.min(start, length));
        const boundedEnd = Math.max(boundedStart, Math.min(end, length));
        const output: SequenceIdRun[] = [];
        collectCausalIdRuns(
          this.#root,
          boundedStart,
          boundedEnd,
          output,
          causalCount,
          isVisible,
        );
        return output;
      },
    };
    this.#cachedCausalViews.push({ version: causalVersion, view });
    if (this.#cachedCausalViews.length > MAX_CACHED_CAUSAL_VIEWS) {
      this.#cachedCausalViews.shift();
    }
    return view;
  }

  #invalidateCausalView(): void {
    this.#cachedCausalViews.length = 0;
  }

  reset(): void {
    this.#invalidateCausalView();
    this.#root = undefined;
    this.#tail = undefined;
    this.#locationsByPeer.clear();
    this.#locationsByLamportPeer.clear();
    this.#nodesByLocationId.length = 0;
    this.#deletionsByPeer.clear();
    this.#scalarDeletionTargetsByPeer.clear();
    this.#deletionRuns.reset();
    this.#maxCounterByPeer.clear();
    this.#hasLazyDeletions = false;
    this.#structureVersion += 1;
  }

  reservePeerCounters(peer: bigint, endExclusive: number): void {
    if (!Number.isSafeInteger(endExclusive) || endExclusive < 0) {
      throw new RangeError("sequence peer counter capacity is out of range");
    }
    let locations = this.#locationsByPeer.get(peer);
    if (locations === undefined) {
      locations = new CounterStore();
      this.#locationsByPeer.set(peer, locations);
    }
    locations.reserveDense(endExclusive);
  }

  #newNode(
    storage: SequenceNodeStorage<T> | readonly T[],
    recordNew: boolean,
  ): SequenceNode<T> {
    const element = Array.isArray(storage)
      ? storage.length === 1
        ? storage[0]!
        : [...storage]
      : (storage as SequenceNodeStorage<T>);
    const isSpan = element instanceof SequenceSpan;
    const length = isSpan
      ? (element as SequenceSpan<T>).length
      : Array.isArray(element)
        ? element.length
        : 1;
    if (length === 0 || length > MAX_SEQUENCE_SPAN) {
      throw new RangeError("sequence span length is out of range");
    }
    this.#randomState ^= this.#randomState << 13;
    this.#randomState ^= this.#randomState >>> 17;
    this.#randomState ^= this.#randomState << 5;
    const priority = this.#randomState >>> 0;
    const locationId = this.#nodesByLocationId.length;
    const node: SequenceNode<T> = {
      element,
      isSpan,
      ownCount: length,
      locationId,
      priority,
      left: undefined,
      right: undefined,
      parent: undefined,
      ownVisibleCount: 0,
      ownVisibleUtf16: 0,
      ownVisibleUtf8: 0,
      ownFirstVisibleId: undefined,
      ownLastVisibleId: undefined,
      ownVisibleIdRunCount: 0,
      ownFirstId: { peer: 0n, counter: 0 },
      ownLastId: { peer: 0n, counter: 0 },
      ownIdRunCount: 0,
      ownUtf16: 0,
      ownUtf8: 0,
      visibleOffsets: undefined,
      visibleUtf16Prefix: undefined,
      visibleUtf8Prefix: undefined,
      allCount: 0,
      visibleCount: 0,
      visibleUtf16: 0,
      visibleUtf8: 0,
      firstVisibleId: undefined,
      lastVisibleId: undefined,
      visibleIdRunCount: 0,
      firstId: { peer: 0n, counter: 0 },
      lastId: { peer: 0n, counter: 0 },
      idRunCount: 0,
      allUtf16: 0,
      allUtf8: 0,
      lazyDeleted: false,
      lazyVisible: false,
    };
    this.#nodesByLocationId.push(node);
    recomputeOwn(node, this.#metrics);
    recompute(node, this.#metrics);
    if (recordNew) this.#recordNewElements(node);
    else this.#assignLocations(node);
    return node;
  }

  #recordNewElements(node: SequenceNode<T>): void {
    for (let offset = 0; offset < nodeLength(node); offset += 1) {
      this.#recordNewElement(node, offset);
    }
  }

  #recordNewElement(node: SequenceNode<T>, offset: number): void {
    if (!node.isSpan) {
      this.#recordNewPlainElement(node, offset, nodeElement(node, offset));
      return;
    }
    const idPeer = nodePeer(node, offset);
    const idCounter = nodeCounter(node, offset);
    const location = node.locationId * SEQUENCE_LOCATION_STRIDE + offset;
    let peer = this.#locationsByPeer.get(idPeer);
    if (peer === undefined) {
      peer = new CounterStore();
      this.#locationsByPeer.set(idPeer, peer);
    }
    peer.set(idCounter, location);
    const lamport = nodeLamport(node, offset);
    if (lamport !== undefined) {
      this.#locationsByLamportPeer.get(idPeer)?.set(lamport, location);
    }
    const operationEnd = idCounter + 1;
    const previousOperationEnd = this.#maxCounterByPeer.get(idPeer) ?? 0;
    if (operationEnd > previousOperationEnd) {
      this.#maxCounterByPeer.set(idPeer, operationEnd);
    }
    const deletionIds = (node.element as SequenceSpan<T>).deletionIdsAt(offset);
    if (deletionIds.length > 0) {
      const element = nodeElement(node, offset);
      for (const deletionId of deletionIds) {
        this.#recordDeletionElement(element, deletionId);
        this.recordOperationId(deletionId);
      }
    }
  }

  #recordNewPlainElement(node: SequenceNode<T>, offset: number, element: T): void {
    setElementLocation(element, node, offset);
    let peer = this.#locationsByPeer.get(element.id.peer);
    if (peer === undefined) {
      peer = new CounterStore();
      this.#locationsByPeer.set(element.id.peer, peer);
    }
    peer.set(element.id.counter, element);
    if (element.lamport !== undefined) {
      this.#locationsByLamportPeer.get(element.id.peer)?.set(element.lamport, element);
    }
    const operationEnd = element.id.counter + 1;
    const previousOperationEnd = this.#maxCounterByPeer.get(element.id.peer) ?? 0;
    if (operationEnd > previousOperationEnd) {
      this.#maxCounterByPeer.set(element.id.peer, operationEnd);
    }
    for (const deletionId of sequenceDeletionIds(element)) {
      this.#recordDeletionElement(element, deletionId);
      this.recordOperationId(deletionId);
    }
  }

  #assignLocations(node: SequenceNode<T>): void {
    if (node.isSpan) {
      (node.element as SequenceSpan<T>).forEachRetained((element, offset) => {
        setElementLocation(element, node, offset);
      });
    } else {
      for (let offset = 0; offset < nodeLength(node); offset += 1) {
        const element = nodeElement(node, offset);
        if (elementNode(element) === undefined) {
          throw new Error("cannot relocate an unindexed sequence element");
        }
        setElementLocation(element, node, offset);
      }
    }
    for (let offset = 0; offset < nodeLength(node); offset += 1) {
      const peer = nodePeer(node, offset);
      const counter = nodeCounter(node, offset);
      const location: SequenceLocation<T> = node.isSpan
        ? node.locationId * SEQUENCE_LOCATION_STRIDE + offset
        : nodeElement(node, offset);
      this.#locationsByPeer.get(peer)?.set(counter, location);
      const lamport = nodeLamport(node, offset);
      if (lamport !== undefined) {
        this.#locationsByLamportPeer.get(peer)?.set(lamport, location);
      }
    }
  }

  #nodeAtLocation(location: number): SequenceNode<T> {
    const node = this.#nodesByLocationId[Math.floor(location / SEQUENCE_LOCATION_STRIDE)];
    if (node === undefined) throw new Error("sequence location points to a missing node");
    return node;
  }

  #elementAtLocation(location: number): T {
    return nodeElement(
      this.#nodeAtLocation(location),
      location % SEQUENCE_LOCATION_STRIDE,
    );
  }

  #elementAtDeletionLocation(location: DeletionLocation<T>): T {
    if ("stableSequenceId" in location) {
      const element = this.findById(location);
      if (element === undefined) throw new Error("deleted sequence element is missing");
      return element;
    }
    return location;
  }

  #recordDeletionElement(element: T, id: SequenceId): void {
    const node = elementNode(element);
    const offset = elementOffset(element);
    const location: DeletionLocation<T> =
      node?.isSpan === true && offset !== undefined
        ? {
            stableSequenceId: true,
            peer: element.id.peer,
            counter: element.id.counter,
          }
        : element;
    let targets = this.#scalarDeletionTargetsByPeer.get(element.id.peer);
    if (targets === undefined) {
      targets = new PagedCounterStore();
      this.#scalarDeletionTargetsByPeer.set(element.id.peer, targets);
    }
    targets.set(element.id.counter, location);
    let counters = this.#deletionsByPeer.get(id.peer);
    if (counters === undefined) {
      counters = new CounterStore();
      this.#deletionsByPeer.set(id.peer, counters);
    }
    const elementOrSet = counters.get(id.counter);
    if (elementOrSet === undefined) {
      counters.set(id.counter, location);
    } else if (elementOrSet instanceof Set) {
      elementOrSet.add(location);
    } else if (elementOrSet !== location) {
      counters.set(id.counter, new Set([elementOrSet, location]));
    }
  }

  #split(
    root: SequenceNode<T> | undefined,
    count: number,
  ): [SequenceNode<T> | undefined, SequenceNode<T> | undefined] {
    if (root === undefined) return [undefined, undefined];
    pushNodeDeletion(root, this.#metrics);
    const leftCount = allCount(root.left);
    const ownEnd = leftCount + nodeLength(root);
    if (count < leftCount) {
      const [left, right] = this.#split(root.left, count);
      root.left = right;
      recompute(root, this.#metrics);
      root.parent = undefined;
      if (left !== undefined) left.parent = undefined;
      return [left, root];
    }
    if (count > ownEnd) {
      const [left, right] = this.#split(root.right, count - ownEnd);
      root.right = left;
      recompute(root, this.#metrics);
      root.parent = undefined;
      if (right !== undefined) right.parent = undefined;
      return [root, right];
    }
    if (count === leftCount) {
      const left = root.left;
      root.left = undefined;
      recompute(root, this.#metrics);
      root.parent = undefined;
      if (left !== undefined) left.parent = undefined;
      return [left, root];
    }
    if (count === ownEnd) {
      const right = root.right;
      root.right = undefined;
      recompute(root, this.#metrics);
      root.parent = undefined;
      if (right !== undefined) right.parent = undefined;
      return [root, right];
    }
    const ownOffset = count - leftCount;
    const storage = root.element;
    const oldRight = root.right;
    root.element = sliceNodeStorage(storage, 0, ownOffset);
    root.ownCount = ownOffset;
    root.right = undefined;
    recomputeOwn(root, this.#metrics);
    recompute(root, this.#metrics);
    root.parent = undefined;
    this.#assignLocations(root);

    const rightRoot = this.#newNode(
      sliceNodeStorage(storage, ownOffset, nodeLengthFromStorage(storage)),
      false,
    );
    const right = merge(rightRoot, oldRight, this.#metrics);
    if (right !== undefined) right.parent = undefined;
    return [root, right];
  }

  #appendToPreviousNode(position: number, elements: readonly T[]): boolean {
    if (position === 0) return false;
    const previous = this.atPhysical(position - 1);
    if (previous === undefined) return false;
    const node = elementNode(previous)!;
    const offset = elementOffset(previous)!;
    const length = nodeLength(node);
    if (
      node.isSpan ||
      offset !== length - 1 ||
      length + elements.length > MAX_SEQUENCE_SPAN
    ) {
      return false;
    }

    const appendingAtTail = position === this.allLength;
    const storage = node.element as T | T[];
    const values = Array.isArray(storage) ? storage : [storage];
    node.element = values;
    const visibleOffsets = node.visibleOffsets ?? [0, node.ownVisibleCount];
    const visibleUtf16Prefix = node.visibleUtf16Prefix ?? [0, node.ownVisibleUtf16];
    const visibleUtf8Prefix = node.visibleUtf8Prefix ?? [0, node.ownVisibleUtf8];
    for (let offset = 0; offset < elements.length; offset += 1) {
      const element = elements[offset]!;
      values.push(element);
      this.#recordNewElement(node, length + offset);
      if (!sequenceIdsContinue(node.ownLastId, element.id)) {
        node.ownIdRunCount += 1;
      }
      node.ownLastId = element.id;
      const metrics = this.#metrics(element);
      node.ownUtf16 += metrics.utf16;
      node.ownUtf8 += metrics.utf8;
      if (!element.deleted) {
        if (node.ownVisibleCount === 0) {
          node.ownFirstVisibleId = element.id;
          node.ownVisibleIdRunCount = 1;
        } else if (!sequenceIdsContinue(node.ownLastVisibleId!, element.id)) {
          node.ownVisibleIdRunCount += 1;
        }
        node.ownLastVisibleId = element.id;
        node.ownVisibleCount += 1;
        node.ownVisibleUtf16 += metrics.utf16;
        node.ownVisibleUtf8 += metrics.utf8;
      }
      visibleOffsets.push(node.ownVisibleCount);
      visibleUtf16Prefix.push(node.ownVisibleUtf16);
      visibleUtf8Prefix.push(node.ownVisibleUtf8);
    }
    node.ownCount += elements.length;
    node.visibleOffsets = visibleOffsets;
    node.visibleUtf16Prefix = visibleUtf16Prefix;
    node.visibleUtf8Prefix = visibleUtf8Prefix;
    recomputeToRoot(node, this.#metrics);
    if (appendingAtTail) this.#tail = node;
    return true;
  }

  #appendSpanToPreviousNode(position: number, span: SequenceSpan<T>): boolean {
    if (position === 0) return false;
    const previous = this.atPhysical(position - 1);
    if (previous === undefined) return false;
    const node = elementNode(previous)!;
    const offset = elementOffset(previous)!;
    const length = nodeLength(node);
    if (
      !node.isSpan ||
      offset !== length - 1 ||
      length + span.length > MAX_SEQUENCE_SPAN ||
      !(node.element as SequenceSpan<T>).append(span)
    ) {
      return false;
    }
    node.ownCount += span.length;

    const appendingAtTail = position === this.allLength;
    const visibleOffsets = node.visibleOffsets ?? [0, node.ownVisibleCount];
    const visibleUtf16Prefix = node.visibleUtf16Prefix ?? [0, node.ownVisibleUtf16];
    const visibleUtf8Prefix = node.visibleUtf8Prefix ?? [0, node.ownVisibleUtf8];
    for (let insertedOffset = 0; insertedOffset < span.length; insertedOffset += 1) {
      const offsetInNode = length + insertedOffset;
      this.#recordNewElement(node, offsetInNode);
      const id = nodeId(node, offsetInNode);
      if (!sequenceIdsContinue(node.ownLastId, id)) node.ownIdRunCount += 1;
      node.ownLastId = id;
      const value = nodeMetrics(node, offsetInNode, this.#metrics);
      node.ownUtf16 += value.utf16;
      node.ownUtf8 += value.utf8;
      if (!nodeDeleted(node, offsetInNode)) {
        if (node.ownVisibleCount === 0) {
          node.ownFirstVisibleId = id;
          node.ownVisibleIdRunCount = 1;
        } else if (!sequenceIdsContinue(node.ownLastVisibleId!, id)) {
          node.ownVisibleIdRunCount += 1;
        }
        node.ownLastVisibleId = id;
        node.ownVisibleCount += 1;
        node.ownVisibleUtf16 += value.utf16;
        node.ownVisibleUtf8 += value.utf8;
      }
      visibleOffsets.push(node.ownVisibleCount);
      visibleUtf16Prefix.push(node.ownVisibleUtf16);
      visibleUtf8Prefix.push(node.ownVisibleUtf8);
    }
    node.visibleOffsets = visibleOffsets;
    node.visibleUtf16Prefix = visibleUtf16Prefix;
    node.visibleUtf8Prefix = visibleUtf8Prefix;
    recomputeToRoot(node, this.#metrics);
    if (appendingAtTail) this.#tail = node;
    return true;
  }

  #insertSingleNode(
    root: SequenceNode<T> | undefined,
    position: number,
    inserted: SequenceNode<T>,
  ): SequenceNode<T> {
    if (root === undefined) {
      inserted.parent = undefined;
      return inserted;
    }
    pushNodeDeletion(root, this.#metrics);
    if (inserted.priority < root.priority) {
      const [left, right] = this.#split(root, position);
      inserted.left = left;
      inserted.right = right;
      recompute(inserted, this.#metrics);
      inserted.parent = undefined;
      return inserted;
    }

    const leftCount = allCount(root.left);
    const ownEnd = leftCount + nodeLength(root);
    if (position <= leftCount) {
      root.left = this.#insertSingleNode(root.left, position, inserted);
      recompute(root, this.#metrics);
      root.parent = undefined;
      return root;
    }
    if (position >= ownEnd) {
      root.right = this.#insertSingleNode(root.right, position - ownEnd, inserted);
      recompute(root, this.#metrics);
      root.parent = undefined;
      return root;
    }

    const [left, right] = this.#split(root, position);
    return merge(merge(left, inserted, this.#metrics), right, this.#metrics)!;
  }

  #appendAtTail(
    left: SequenceNode<T> | undefined,
    right: SequenceNode<T> | undefined,
  ): SequenceNode<T> | undefined {
    if (left === undefined || right === undefined) {
      if (right !== undefined) this.#tail = rightmost(right);
      return merge(left, right, this.#metrics);
    }
    this.#tail = rightmost(right);
    return merge(left, right, this.#metrics);
  }
}

export function someSequenceDeletion(
  element: IndexedSequenceElement,
  predicate: (id: SequenceId) => boolean,
): boolean {
  if (
    element.deletedByPeer !== undefined &&
    element.deletedByCounter !== undefined &&
    predicate({ peer: element.deletedByPeer, counter: element.deletedByCounter })
  ) {
    return true;
  }
  return element.deletedBy?.some(predicate) ?? false;
}

function sequenceDeletionIds(element: IndexedSequenceElement): SequenceId[] {
  const ids = element.deletedBy === undefined ? [] : [...element.deletedBy];
  if (element.deletedByPeer !== undefined && element.deletedByCounter !== undefined) {
    ids.unshift({ peer: element.deletedByPeer, counter: element.deletedByCounter });
  }
  return ids;
}

export function everySequenceDeletion(
  element: IndexedSequenceElement,
  predicate: (id: SequenceId) => boolean,
): boolean {
  if (
    element.deletedByPeer !== undefined &&
    element.deletedByCounter !== undefined &&
    !predicate({ peer: element.deletedByPeer, counter: element.deletedByCounter })
  ) {
    return false;
  }
  return element.deletedBy?.every(predicate) ?? true;
}

function elementNode<T extends IndexedSequenceElement>(
  element: T,
): SequenceNode<T> | undefined {
  return (element as LocatedSequenceElement<T>)[SEQUENCE_NODE];
}

function elementOffset<T extends IndexedSequenceElement>(element: T): number | undefined {
  return (element as LocatedSequenceElement<T>)[SEQUENCE_OFFSET];
}

function setElementLocation<T extends IndexedSequenceElement>(
  element: T,
  node: SequenceNode<T>,
  offset: number,
): void {
  const located = element as LocatedSequenceElement<T>;
  located[SEQUENCE_NODE] = node;
  located[SEQUENCE_OFFSET] = offset;
}

function nodeLength<T extends IndexedSequenceElement>(node: SequenceNode<T>): number {
  return node.ownCount;
}

function nodeLengthFromStorage<T extends IndexedSequenceElement>(
  storage: SequenceNodeStorage<T>,
): number {
  return storage instanceof SequenceSpan
    ? storage.length
    : Array.isArray(storage)
      ? storage.length
      : 1;
}

function nodeElement<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  offset: number,
): T {
  if (node.isSpan) {
    const element = (node.element as SequenceSpan<T>).elementAt(offset);
    setElementLocation(element, node, offset);
    return element;
  }
  const storage = node.element as T | T[];
  return Array.isArray(storage) ? storage[offset]! : storage;
}

function nodeId<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  offset: number,
): SequenceId {
  return node.isSpan
    ? (node.element as SequenceSpan<T>).idAt(offset)
    : Array.isArray(node.element)
      ? (node.element as T[])[offset]!.id
      : (node.element as T).id;
}

function nodePeer<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  offset: number,
): bigint {
  return node.isSpan
    ? (node.element as SequenceSpan<T>).peerAt(offset)
    : Array.isArray(node.element)
      ? (node.element as T[])[offset]!.id.peer
      : (node.element as T).id.peer;
}

function nodeCounter<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  offset: number,
): number {
  return node.isSpan
    ? (node.element as SequenceSpan<T>).counterAt(offset)
    : Array.isArray(node.element)
      ? (node.element as T[])[offset]!.id.counter
      : (node.element as T).id.counter;
}

function findNextIncludedPhysical<T extends IndexedSequenceElement>(
  node: SequenceNode<T> | undefined,
  nodeStart: number,
  start: number,
  version: ReadonlyMap<bigint, number>,
): { readonly element: T; readonly index: number } | undefined {
  if (node === undefined || nodeStart + node.allCount <= start) return undefined;
  if (
    node.idRunCount === 1 &&
    (version.get(node.firstId.peer) ?? 0) <= node.firstId.counter
  ) {
    return undefined;
  }

  const leftCount = allCount(node.left);
  const ownStart = nodeStart + leftCount;
  const fromLeft = findNextIncludedPhysical(node.left, nodeStart, start, version);
  if (fromLeft !== undefined) return fromLeft;

  const offsetStart = Math.max(0, start - ownStart);
  if (offsetStart < nodeLength(node)) {
    if (node.ownIdRunCount === 1) {
      const peer = nodePeer(node, offsetStart);
      const counter = nodeCounter(node, offsetStart);
      if (counter < (version.get(peer) ?? 0)) {
        return {
          element: nodeElement(node, offsetStart),
          index: ownStart + offsetStart,
        };
      }
    } else {
      for (let offset = offsetStart; offset < nodeLength(node); offset += 1) {
        const peer = nodePeer(node, offset);
        const counter = nodeCounter(node, offset);
        if (counter < (version.get(peer) ?? 0)) {
          return { element: nodeElement(node, offset), index: ownStart + offset };
        }
      }
    }
  }

  return findNextIncludedPhysical(
    node.right,
    ownStart + nodeLength(node),
    start,
    version,
  );
}

function nodeLamport<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  offset: number,
): number | undefined {
  return node.isSpan
    ? (node.element as SequenceSpan<T>).lamportAt(offset)
    : Array.isArray(node.element)
      ? (node.element as T[])[offset]!.lamport
      : (node.element as T).lamport;
}

function nodeDeleted<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  offset: number,
): boolean {
  return node.isSpan
    ? (node.element as SequenceSpan<T>).deletedAt(offset)
    : Array.isArray(node.element)
      ? (node.element as T[])[offset]!.deleted
      : (node.element as T).deleted;
}

function setNodeDeleted<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  offset: number,
  deleted: boolean,
): void {
  if (node.isSpan) {
    (node.element as SequenceSpan<T>).setDeletedAt(offset, deleted);
  } else if (Array.isArray(node.element)) {
    (node.element as T[])[offset]!.deleted = deleted;
  } else {
    (node.element as T).deleted = deleted;
  }
}

function nodeMetrics<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  offset: number,
  metrics: (element: T) => SequenceMetrics,
): SequenceMetrics {
  return node.isSpan
    ? (node.element as SequenceSpan<T>).metricsAt(offset)
    : metrics(
        Array.isArray(node.element)
          ? (node.element as T[])[offset]!
          : (node.element as T),
      );
}

function sliceNodeStorage<T extends IndexedSequenceElement>(
  storage: SequenceNodeStorage<T>,
  start: number,
  end: number,
): SequenceNodeStorage<T> {
  if (storage instanceof SequenceSpan) return storage.slice(start, end);
  const elements = Array.isArray(storage) ? storage.slice(start, end) : [storage];
  return elements.length === 1 ? elements[0]! : elements;
}

function appendNodeElements<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  output: T[],
): void {
  if (node.isSpan) {
    const span = node.element as SequenceSpan<T>;
    for (let offset = 0; offset < span.length; offset += 1) {
      output.push(nodeElement(node, offset));
    }
  } else if (Array.isArray(node.element)) output.push(...(node.element as T[]));
  else output.push(node.element as T);
}

function allCount<T extends IndexedSequenceElement>(
  node: SequenceNode<T> | undefined,
): number {
  return node?.allCount ?? 0;
}

function visibleCount<T extends IndexedSequenceElement>(
  node: SequenceNode<T> | undefined,
): number {
  return node?.visibleCount ?? 0;
}

function visibleMetric<T extends IndexedSequenceElement>(
  node: SequenceNode<T> | undefined,
  metric: Metric,
): number {
  if (node === undefined) return 0;
  return metric === "utf16" ? node.visibleUtf16 : node.visibleUtf8;
}

function ownVisibleCount<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
): number {
  return node.ownVisibleCount;
}

function visibleElementsBefore<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  offset: number,
): number {
  if (node.visibleOffsets !== undefined) return node.visibleOffsets[offset]!;
  let count = 0;
  for (let index = 0; index < offset; index += 1) {
    if (!nodeDeleted(node, index)) count += 1;
  }
  return count;
}

function ownVisibleMetric<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  metric: Metric,
  _metrics: (element: T) => SequenceMetrics,
): number {
  return metric === "utf16" ? node.ownVisibleUtf16 : node.ownVisibleUtf8;
}

function visibleMetricBefore<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  offset: number,
  metric: Metric,
  metrics: (element: T) => SequenceMetrics,
): number {
  const prefix = metric === "utf16" ? node.visibleUtf16Prefix : node.visibleUtf8Prefix;
  if (prefix !== undefined) return prefix[offset]!;
  let total = 0;
  for (let index = 0; index < offset; index += 1) {
    if (!nodeDeleted(node, index)) total += nodeMetrics(node, index, metrics)[metric];
  }
  return total;
}

function physicalOffsetAtVisibleIndex<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  index: number,
): number {
  const prefix = node.visibleOffsets;
  if (prefix === undefined) return 0;
  let low = 1;
  let high = prefix.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (prefix[middle]! > index) high = middle;
    else low = middle + 1;
  }
  return low - 1;
}

function physicalOffsetAtMetricOffset<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  offset: number,
  metric: Metric,
): number | undefined {
  const prefix = metric === "utf16" ? node.visibleUtf16Prefix : node.visibleUtf8Prefix;
  if (prefix === undefined) {
    if (offset === 0) return 0;
    return offset === ownVisibleMetric(node, metric, listMetrics) ? 1 : undefined;
  }
  let low = 0;
  let high = prefix.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (prefix[middle]! >= offset) high = middle;
    else low = middle + 1;
  }
  return prefix[low] === offset ? low : undefined;
}

function updateNodeElementVisibility<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  offset: number,
  element: T,
  deleted: boolean,
  metrics: (element: T) => SequenceMetrics,
): void {
  const value = metrics(element);
  const direction = deleted ? -1 : 1;
  setNodeDeleted(node, offset, deleted);
  node.ownVisibleCount += direction;
  node.ownVisibleUtf16 += direction * value.utf16;
  node.ownVisibleUtf8 += direction * value.utf8;
  recomputeOwnIdRuns(node);
  if (nodeLength(node) === 1) return;
  for (let index = offset + 1; index < node.visibleOffsets!.length; index += 1) {
    node.visibleOffsets![index]! += direction;
    node.visibleUtf16Prefix![index]! += direction * value.utf16;
    node.visibleUtf8Prefix![index]! += direction * value.utf8;
  }
}

function recomputeOwn<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  metrics: (element: T) => SequenceMetrics,
): void {
  if (!node.isSpan) {
    recomputeOwnElements(node, metrics);
    return;
  }
  node.ownFirstId = nodeId(node, 0);
  node.ownLastId = nodeId(node, nodeLength(node) - 1);
  node.ownIdRunCount = 1;
  for (let offset = 1; offset < nodeLength(node); offset += 1) {
    if (!sequenceIdsContinue(nodeId(node, offset - 1), nodeId(node, offset))) {
      node.ownIdRunCount += 1;
    }
  }
  if (nodeLength(node) === 1) {
    const value = nodeMetrics(node, 0, metrics);
    node.ownUtf16 = value.utf16;
    node.ownUtf8 = value.utf8;
    node.visibleOffsets = undefined;
    node.visibleUtf16Prefix = undefined;
    node.visibleUtf8Prefix = undefined;
    if (nodeDeleted(node, 0)) {
      node.ownVisibleCount = 0;
      node.ownVisibleUtf16 = 0;
      node.ownVisibleUtf8 = 0;
      node.ownFirstVisibleId = undefined;
      node.ownLastVisibleId = undefined;
      node.ownVisibleIdRunCount = 0;
    } else {
      node.ownVisibleCount = 1;
      node.ownVisibleUtf16 = value.utf16;
      node.ownVisibleUtf8 = value.utf8;
      node.ownFirstVisibleId = nodeId(node, 0);
      node.ownLastVisibleId = nodeId(node, 0);
      node.ownVisibleIdRunCount = 1;
    }
    return;
  }
  node.ownVisibleCount = 0;
  node.ownVisibleUtf16 = 0;
  node.ownVisibleUtf8 = 0;
  node.ownUtf16 = 0;
  node.ownUtf8 = 0;
  node.ownFirstVisibleId = undefined;
  node.ownLastVisibleId = undefined;
  node.ownVisibleIdRunCount = 0;
  const visibleOffsets = [0];
  const visibleUtf16Prefix = [0];
  const visibleUtf8Prefix = [0];
  for (let offset = 0; offset < nodeLength(node); offset += 1) {
    const id = nodeId(node, offset);
    const value = nodeMetrics(node, offset, metrics);
    node.ownUtf16 += value.utf16;
    node.ownUtf8 += value.utf8;
    if (!nodeDeleted(node, offset)) {
      if (node.ownVisibleCount === 0) {
        node.ownFirstVisibleId = id;
        node.ownVisibleIdRunCount = 1;
      } else if (!sequenceIdsContinue(node.ownLastVisibleId!, id)) {
        node.ownVisibleIdRunCount += 1;
      }
      node.ownLastVisibleId = id;
      node.ownVisibleCount += 1;
      node.ownVisibleUtf16 += value.utf16;
      node.ownVisibleUtf8 += value.utf8;
    }
    visibleOffsets.push(node.ownVisibleCount);
    visibleUtf16Prefix.push(node.ownVisibleUtf16);
    visibleUtf8Prefix.push(node.ownVisibleUtf8);
  }
  node.visibleOffsets = visibleOffsets;
  node.visibleUtf16Prefix = visibleUtf16Prefix;
  node.visibleUtf8Prefix = visibleUtf8Prefix;
}

function recomputeOwnElements<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  metrics: (element: T) => SequenceMetrics,
): void {
  const storage = node.element as T | T[];
  const elements = Array.isArray(storage) ? storage : undefined;
  const first = elements?.[0] ?? (storage as T);
  const last = elements?.at(-1) ?? (storage as T);
  node.ownFirstId = first.id;
  node.ownLastId = last.id;
  node.ownIdRunCount = 1;
  if (elements !== undefined) {
    for (let offset = 1; offset < elements.length; offset += 1) {
      if (!sequenceIdsContinue(elements[offset - 1]!.id, elements[offset]!.id)) {
        node.ownIdRunCount += 1;
      }
    }
  }
  if (elements === undefined) {
    const value = metrics(storage as T);
    node.ownUtf16 = value.utf16;
    node.ownUtf8 = value.utf8;
    node.visibleOffsets = undefined;
    node.visibleUtf16Prefix = undefined;
    node.visibleUtf8Prefix = undefined;
    if ((storage as T).deleted) {
      node.ownVisibleCount = 0;
      node.ownVisibleUtf16 = 0;
      node.ownVisibleUtf8 = 0;
      node.ownFirstVisibleId = undefined;
      node.ownLastVisibleId = undefined;
      node.ownVisibleIdRunCount = 0;
    } else {
      node.ownVisibleCount = 1;
      node.ownVisibleUtf16 = value.utf16;
      node.ownVisibleUtf8 = value.utf8;
      node.ownFirstVisibleId = (storage as T).id;
      node.ownLastVisibleId = (storage as T).id;
      node.ownVisibleIdRunCount = 1;
    }
    return;
  }
  node.ownVisibleCount = 0;
  node.ownVisibleUtf16 = 0;
  node.ownVisibleUtf8 = 0;
  node.ownUtf16 = 0;
  node.ownUtf8 = 0;
  node.ownFirstVisibleId = undefined;
  node.ownLastVisibleId = undefined;
  node.ownVisibleIdRunCount = 0;
  const visibleOffsets = [0];
  const visibleUtf16Prefix = [0];
  const visibleUtf8Prefix = [0];
  for (const element of elements) {
    const value = metrics(element);
    node.ownUtf16 += value.utf16;
    node.ownUtf8 += value.utf8;
    if (!element.deleted) {
      if (node.ownVisibleCount === 0) {
        node.ownFirstVisibleId = element.id;
        node.ownVisibleIdRunCount = 1;
      } else if (!sequenceIdsContinue(node.ownLastVisibleId!, element.id)) {
        node.ownVisibleIdRunCount += 1;
      }
      node.ownLastVisibleId = element.id;
      node.ownVisibleCount += 1;
      node.ownVisibleUtf16 += value.utf16;
      node.ownVisibleUtf8 += value.utf8;
    }
    visibleOffsets.push(node.ownVisibleCount);
    visibleUtf16Prefix.push(node.ownVisibleUtf16);
    visibleUtf8Prefix.push(node.ownVisibleUtf8);
  }
  node.visibleOffsets = visibleOffsets;
  node.visibleUtf16Prefix = visibleUtf16Prefix;
  node.visibleUtf8Prefix = visibleUtf8Prefix;
}

function recomputeVisibility<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
): void {
  const left = node.left;
  const right = node.right;
  node.visibleCount =
    (left?.visibleCount ?? 0) + node.ownVisibleCount + (right?.visibleCount ?? 0);
  node.visibleUtf16 =
    (left?.visibleUtf16 ?? 0) + node.ownVisibleUtf16 + (right?.visibleUtf16 ?? 0);
  node.visibleUtf8 =
    (left?.visibleUtf8 ?? 0) + node.ownVisibleUtf8 + (right?.visibleUtf8 ?? 0);
  node.firstVisibleId =
    left?.firstVisibleId ?? node.ownFirstVisibleId ?? right?.firstVisibleId;
  node.lastVisibleId =
    right?.lastVisibleId ?? node.ownLastVisibleId ?? left?.lastVisibleId;
  node.visibleIdRunCount =
    (left?.visibleIdRunCount ?? 0) +
    node.ownVisibleIdRunCount +
    (right?.visibleIdRunCount ?? 0);
  const leftLastVisible = left?.lastVisibleId;
  const ownFirstVisible = node.ownFirstVisibleId;
  if (
    leftLastVisible !== undefined &&
    ownFirstVisible !== undefined &&
    leftLastVisible.peer === ownFirstVisible.peer &&
    leftLastVisible.counter + 1 === ownFirstVisible.counter
  ) {
    node.visibleIdRunCount -= 1;
  }
  const beforeRight = node.ownLastVisibleId ?? leftLastVisible;
  const rightFirstVisible = right?.firstVisibleId;
  if (
    beforeRight !== undefined &&
    rightFirstVisible !== undefined &&
    beforeRight.peer === rightFirstVisible.peer &&
    beforeRight.counter + 1 === rightFirstVisible.counter
  ) {
    node.visibleIdRunCount -= 1;
  }
}

function recompute<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  _metrics: (element: T) => SequenceMetrics,
): void {
  const left = node.left;
  const right = node.right;
  if (left === undefined && right === undefined) {
    node.allCount = nodeLength(node);
    node.visibleCount = node.ownVisibleCount;
    node.visibleUtf16 = node.ownVisibleUtf16;
    node.visibleUtf8 = node.ownVisibleUtf8;
    node.firstVisibleId = node.ownFirstVisibleId;
    node.lastVisibleId = node.ownLastVisibleId;
    node.visibleIdRunCount = node.ownVisibleIdRunCount;
    node.firstId = node.ownFirstId;
    node.lastId = node.ownLastId;
    node.idRunCount = node.ownIdRunCount;
    node.allUtf16 = node.ownUtf16;
    node.allUtf8 = node.ownUtf8;
    return;
  }
  node.allCount = (left?.allCount ?? 0) + nodeLength(node) + (right?.allCount ?? 0);
  node.allUtf16 = (left?.allUtf16 ?? 0) + node.ownUtf16 + (right?.allUtf16 ?? 0);
  node.allUtf8 = (left?.allUtf8 ?? 0) + node.ownUtf8 + (right?.allUtf8 ?? 0);
  recomputeVisibility(node);
  node.firstId = left?.firstId ?? node.ownFirstId;
  node.lastId = right?.lastId ?? node.ownLastId;
  node.idRunCount =
    (left?.idRunCount ?? 0) + node.ownIdRunCount + (right?.idRunCount ?? 0);
  if (
    left !== undefined &&
    left.lastId.peer === node.ownFirstId.peer &&
    left.lastId.counter + 1 === node.ownFirstId.counter
  ) {
    node.idRunCount -= 1;
  }
  if (
    right !== undefined &&
    node.ownLastId.peer === right.firstId.peer &&
    node.ownLastId.counter + 1 === right.firstId.counter
  ) {
    node.idRunCount -= 1;
  }
  if (left !== undefined) left.parent = node;
  if (right !== undefined) right.parent = node;
}

function applyNodeDeleted<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  metrics: (element: T) => SequenceMetrics,
): void {
  if (node.lazyDeleted && node.visibleCount === 0) return;
  for (let offset = 0; offset < nodeLength(node); offset += 1) {
    setNodeDeleted(node, offset, true);
  }
  recomputeOwn(node, metrics);
  node.visibleCount = 0;
  node.visibleUtf16 = 0;
  node.visibleUtf8 = 0;
  node.firstVisibleId = undefined;
  node.lastVisibleId = undefined;
  node.visibleIdRunCount = 0;
  node.lazyDeleted = true;
  node.lazyVisible = false;
}

function applyNodeVisible<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  metrics: (element: T) => SequenceMetrics,
): void {
  if (node.lazyVisible && node.visibleCount === node.allCount) return;
  for (let offset = 0; offset < nodeLength(node); offset += 1) {
    setNodeDeleted(node, offset, false);
  }
  recomputeOwn(node, metrics);
  node.visibleCount = node.allCount;
  node.visibleUtf16 = node.allUtf16;
  node.visibleUtf8 = node.allUtf8;
  node.firstVisibleId = node.firstId;
  node.lastVisibleId = node.lastId;
  node.visibleIdRunCount = node.idRunCount;
  node.lazyDeleted = false;
  node.lazyVisible = true;
}

function pushNodeDeletion<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  metrics: (element: T) => SequenceMetrics,
): void {
  if (!node.lazyDeleted && !node.lazyVisible) return;
  if (node.left !== undefined) {
    if (node.lazyDeleted) applyNodeDeleted(node.left, metrics);
    else applyNodeVisible(node.left, metrics);
  }
  if (node.right !== undefined) {
    if (node.lazyDeleted) applyNodeDeleted(node.right, metrics);
    else applyNodeVisible(node.right, metrics);
  }
  node.lazyDeleted = false;
  node.lazyVisible = false;
}

function materializeElementDeletion<T extends IndexedSequenceElement>(
  element: T,
  metrics: (element: T) => SequenceMetrics,
): void {
  const node = elementNode(element);
  if (node === undefined) return;
  const path: SequenceNode<T>[] = [];
  let current: SequenceNode<T> | undefined = node;
  while (current !== undefined) {
    path.push(current);
    current = current.parent;
  }
  for (let index = path.length - 1; index >= 0; index -= 1) {
    pushNodeDeletion(path[index]!, metrics);
  }
}

function materializeAllDeletions<T extends IndexedSequenceElement>(
  node: SequenceNode<T> | undefined,
  metrics: (element: T) => SequenceMetrics,
): void {
  if (node === undefined) return;
  pushNodeDeletion(node, metrics);
  materializeAllDeletions(node.left, metrics);
  materializeAllDeletions(node.right, metrics);
}

function recomputeOwnIdRuns<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
): void {
  node.ownFirstVisibleId = undefined;
  node.ownLastVisibleId = undefined;
  node.ownVisibleIdRunCount = 0;
  for (let offset = 0; offset < nodeLength(node); offset += 1) {
    if (nodeDeleted(node, offset)) continue;
    const id = nodeId(node, offset);
    if (node.ownFirstVisibleId === undefined) {
      node.ownFirstVisibleId = id;
      node.ownVisibleIdRunCount = 1;
    } else if (!sequenceIdsContinue(node.ownLastVisibleId!, id)) {
      node.ownVisibleIdRunCount += 1;
    }
    node.ownLastVisibleId = id;
  }
}

function sequenceIdsContinue(left: SequenceId, right: SequenceId): boolean {
  return left.peer === right.peer && left.counter + 1 === right.counter;
}

function normalizeSequenceIdRuns(runs: readonly SequenceIdRun[]): SequenceIdRun[] {
  const sorted = [...runs].sort((left, right) =>
    left.start.peer < right.start.peer
      ? -1
      : left.start.peer > right.start.peer
        ? 1
        : left.start.counter - right.start.counter,
  );
  const output: { start: SequenceId; length: number }[] = [];
  for (const run of sorted) {
    const previous = output.at(-1);
    if (
      previous !== undefined &&
      previous.start.peer === run.start.peer &&
      run.start.counter <= previous.start.counter + previous.length
    ) {
      previous.length =
        Math.max(
          previous.start.counter + previous.length,
          run.start.counter + run.length,
        ) - previous.start.counter;
    } else {
      output.push({ start: { ...run.start }, length: run.length });
    }
  }
  return output;
}

function recomputeToRoot<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  metrics: (element: T) => SequenceMetrics,
  includePhysical = true,
): void {
  let current: SequenceNode<T> | undefined = node;
  while (current !== undefined) {
    if (includePhysical) recompute(current, metrics);
    else recomputeVisibility(current);
    current = current.parent;
  }
}

function recomputeAffected<T extends IndexedSequenceElement>(
  node: SequenceNode<T> | undefined,
  affected: ReadonlySet<SequenceNode<T>>,
  metrics: (element: T) => SequenceMetrics,
): void {
  if (node === undefined || !affected.has(node)) return;
  recomputeAffected(node.left, affected, metrics);
  recomputeAffected(node.right, affected, metrics);
  recompute(node, metrics);
}

function merge<T extends IndexedSequenceElement>(
  left: SequenceNode<T> | undefined,
  right: SequenceNode<T> | undefined,
  metrics: (element: T) => SequenceMetrics,
): SequenceNode<T> | undefined {
  if (left === undefined) {
    if (right !== undefined) right.parent = undefined;
    return right;
  }
  if (right === undefined) {
    left.parent = undefined;
    return left;
  }
  if (left.priority <= right.priority) {
    pushNodeDeletion(left, metrics);
    left.right = merge(left.right, right, metrics);
    recompute(left, metrics);
    left.parent = undefined;
    return left;
  }
  pushNodeDeletion(right, metrics);
  right.left = merge(left, right.left, metrics);
  recompute(right, metrics);
  right.parent = undefined;
  return right;
}

function visitInOrder<T extends IndexedSequenceElement>(
  root: SequenceNode<T> | undefined,
  visit: (node: SequenceNode<T>) => void,
): void {
  const stack: SequenceNode<T>[] = [];
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

function visitVisibleIdRuns<T extends IndexedSequenceElement>(
  node: SequenceNode<T> | undefined,
  start: number,
  end: number,
  metrics: (element: T) => SequenceMetrics,
  visit: (start: SequenceId, length: number) => void,
): void {
  if (node === undefined || start >= end) return;
  const total = visibleCount(node);
  if (
    start === 0 &&
    end === total &&
    node.visibleIdRunCount === 1 &&
    node.firstVisibleId !== undefined
  ) {
    visit(node.firstVisibleId, total);
    return;
  }

  pushNodeDeletion(node, metrics);
  const leftCount = visibleCount(node.left);
  if (start < leftCount) {
    visitVisibleIdRuns(node.left, start, Math.min(end, leftCount), metrics, visit);
  }

  const ownCount = ownVisibleCount(node);
  const ownStart = Math.max(0, start - leftCount);
  const ownEnd = Math.min(ownCount, end - leftCount);
  if (ownStart < ownEnd) {
    if (
      ownStart === 0 &&
      ownEnd === ownCount &&
      node.ownVisibleIdRunCount === 1 &&
      node.ownFirstVisibleId !== undefined
    ) {
      visit(node.ownFirstVisibleId, ownCount);
    } else {
      let visibleOffset = 0;
      for (let offset = 0; offset < nodeLength(node); offset += 1) {
        const element = nodeElement(node, offset);
        if (element.deleted) continue;
        if (visibleOffset >= ownStart && visibleOffset < ownEnd) visit(element.id, 1);
        visibleOffset += 1;
        if (visibleOffset >= ownEnd) break;
      }
    }
  }

  const rightStart = leftCount + ownCount;
  if (end > rightStart) {
    visitVisibleIdRuns(
      node.right,
      Math.max(0, start - rightStart),
      end - rightStart,
      metrics,
      visit,
    );
  }
}

interface CounterRange {
  start: number;
  end: number;
}

function counterRangesByPeer(
  runs: readonly SequenceIdRun[],
): ReadonlyMap<bigint, readonly CounterRange[]> {
  const byPeer = new Map<bigint, CounterRange[]>();
  for (const run of runs) {
    if (run.length <= 0) continue;
    let ranges = byPeer.get(run.start.peer);
    if (ranges === undefined) {
      ranges = [];
      byPeer.set(run.start.peer, ranges);
    }
    ranges.push({ start: run.start.counter, end: run.start.counter + run.length });
  }
  for (const ranges of byPeer.values()) {
    ranges.sort((left, right) => left.start - right.start);
    let write = 0;
    for (const range of ranges) {
      const previous = ranges[write - 1];
      if (previous !== undefined && range.start <= previous.end) {
        previous.end = Math.max(previous.end, range.end);
      } else {
        ranges[write++] = range;
      }
    }
    ranges.length = write;
  }
  return byPeer;
}

function counterRangeRelation(
  targets: ReadonlyMap<bigint, readonly CounterRange[]>,
  peer: bigint,
  start: number,
  end: number,
): -1 | 0 | 1 {
  const ranges = targets.get(peer);
  if (ranges === undefined) return 0;
  let low = 0;
  let high = ranges.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (ranges[middle]!.end <= start) low = middle + 1;
    else high = middle;
  }
  const range = ranges[low];
  if (range === undefined || range.start >= end) return 0;
  return range.start <= start && range.end >= end ? 1 : -1;
}

function visitVisibleMetricRangesForIds<T extends IndexedSequenceElement>(
  node: SequenceNode<T> | undefined,
  base: number,
  metric: Metric,
  targets: ReadonlyMap<bigint, readonly CounterRange[]>,
  metrics: (element: T) => SequenceMetrics,
  visit: (start: number, end: number) => void,
): void {
  if (node === undefined || node.visibleCount === 0) return;
  if (node.visibleIdRunCount === 1 && node.firstVisibleId !== undefined) {
    const relation = counterRangeRelation(
      targets,
      node.firstVisibleId.peer,
      node.firstVisibleId.counter,
      node.firstVisibleId.counter + node.visibleCount,
    );
    if (relation === 0) return;
    if (relation === 1) {
      visit(base, base + visibleMetric(node, metric));
      return;
    }
  }

  pushNodeDeletion(node, metrics);
  visitVisibleMetricRangesForIds(node.left, base, metric, targets, metrics, visit);
  let ownBase = base + visibleMetric(node.left, metric);
  for (let offset = 0; offset < nodeLength(node); offset += 1) {
    const element = nodeElement(node, offset);
    if (element.deleted) continue;
    const length = metrics(element)[metric];
    if (
      counterRangeRelation(
        targets,
        element.id.peer,
        element.id.counter,
        element.id.counter + 1,
      ) === 1
    ) {
      visit(ownBase, ownBase + length);
    }
    ownBase += length;
  }
  visitVisibleMetricRangesForIds(node.right, ownBase, metric, targets, metrics, visit);
}

interface DeleteRunResult {
  readonly found: boolean;
  readonly changed: boolean;
}

function countPhysicalIdsInRanges<T extends IndexedSequenceElement>(
  node: SequenceNode<T> | undefined,
  targets: ReadonlyMap<bigint, readonly CounterRange[]>,
): number {
  if (node === undefined) return 0;
  if (node.idRunCount === 1) {
    const relation = counterRangeRelation(
      targets,
      node.firstId.peer,
      node.firstId.counter,
      node.firstId.counter + node.allCount,
    );
    if (relation === 0) return 0;
    if (relation === 1) return node.allCount;
  }
  let count = countPhysicalIdsInRanges(node.left, targets);
  for (let offset = 0; offset < nodeLength(node); offset += 1) {
    const id = nodeElement(node, offset).id;
    if (counterRangeRelation(targets, id.peer, id.counter, id.counter + 1) === 1) {
      count += 1;
    }
  }
  return count + countPhysicalIdsInRanges(node.right, targets);
}

function markIdRunsDeleted<T extends IndexedSequenceElement>(
  node: SequenceNode<T> | undefined,
  targets: ReadonlyMap<bigint, readonly CounterRange[]>,
  metrics: (element: T) => SequenceMetrics,
): DeleteRunResult {
  if (node === undefined) return { found: false, changed: false };
  if (node.idRunCount === 1) {
    const relation = counterRangeRelation(
      targets,
      node.firstId.peer,
      node.firstId.counter,
      node.firstId.counter + node.allCount,
    );
    if (relation === 0) return { found: false, changed: false };
    if (relation === 1) {
      const changed = node.visibleCount > 0;
      applyNodeDeleted(node, metrics);
      return { found: true, changed };
    }
  }

  pushNodeDeletion(node, metrics);
  const left = markIdRunsDeleted(node.left, targets, metrics);
  let ownFound = false;
  let ownChanged = false;
  for (let offset = 0; offset < nodeLength(node); offset += 1) {
    const element = nodeElement(node, offset);
    if (
      counterRangeRelation(
        targets,
        element.id.peer,
        element.id.counter,
        element.id.counter + 1,
      ) !== 1
    ) {
      continue;
    }
    ownFound = true;
    if (!element.deleted) {
      element.deleted = true;
      ownChanged = true;
    }
  }
  if (ownChanged) recomputeOwn(node, metrics);
  const right = markIdRunsDeleted(node.right, targets, metrics);
  const changed = left.changed || ownChanged || right.changed;
  if (changed) recomputeVisibility(node);
  return {
    found: left.found || ownFound || right.found,
    changed,
  };
}

function markIdRunsVisible<T extends IndexedSequenceElement>(
  node: SequenceNode<T> | undefined,
  targets: ReadonlyMap<bigint, readonly CounterRange[]>,
  metrics: (element: T) => SequenceMetrics,
): DeleteRunResult {
  if (node === undefined) return { found: false, changed: false };
  if (node.idRunCount === 1) {
    const relation = counterRangeRelation(
      targets,
      node.firstId.peer,
      node.firstId.counter,
      node.firstId.counter + node.allCount,
    );
    if (relation === 0) return { found: false, changed: false };
    if (relation === 1) {
      const changed = node.visibleCount < node.allCount;
      applyNodeVisible(node, metrics);
      return { found: true, changed };
    }
  }

  pushNodeDeletion(node, metrics);
  const left = markIdRunsVisible(node.left, targets, metrics);
  let ownFound = false;
  let ownChanged = false;
  for (let offset = 0; offset < nodeLength(node); offset += 1) {
    const element = nodeElement(node, offset);
    if (
      counterRangeRelation(
        targets,
        element.id.peer,
        element.id.counter,
        element.id.counter + 1,
      ) !== 1
    ) {
      continue;
    }
    ownFound = true;
    if (element.deleted) {
      element.deleted = false;
      ownChanged = true;
    }
  }
  if (ownChanged) recomputeOwn(node, metrics);
  const right = markIdRunsVisible(node.right, targets, metrics);
  const changed = left.changed || ownChanged || right.changed;
  if (changed) recomputeVisibility(node);
  return {
    found: left.found || ownFound || right.found,
    changed,
  };
}

function visitVisibleRange<T extends IndexedSequenceElement>(
  node: SequenceNode<T> | undefined,
  start: number,
  end: number,
  metrics: (element: T) => SequenceMetrics,
  visit: (element: T) => boolean | void,
): boolean {
  if (node === undefined || start >= end) return true;
  pushNodeDeletion(node, metrics);
  const leftCount = visibleCount(node.left);
  if (start < leftCount) {
    if (!visitVisibleRange(node.left, start, Math.min(end, leftCount), metrics, visit)) {
      return false;
    }
  }
  let ownOffset = leftCount;
  for (let offset = 0; offset < nodeLength(node); offset += 1) {
    const element = nodeElement(node, offset);
    if (element.deleted) continue;
    if (start <= ownOffset && end > ownOffset && visit(element) === false) {
      return false;
    }
    ownOffset += 1;
    if (ownOffset >= end) break;
  }
  if (end > ownOffset) {
    return visitVisibleRange(
      node.right,
      Math.max(0, start - ownOffset),
      end - ownOffset,
      metrics,
      visit,
    );
  }
  return true;
}

function collectCausalRange<T extends IndexedSequenceElement>(
  node: SequenceNode<T> | undefined,
  start: number,
  end: number,
  output: T[],
  count: (node: SequenceNode<T> | undefined) => number,
  isVisible: (element: T) => boolean,
  metrics: (element: T) => SequenceMetrics,
): void {
  if (node === undefined || start >= end) return;
  pushNodeDeletion(node, metrics);
  const leftCount = count(node.left);
  if (start < leftCount) {
    collectCausalRange(
      node.left,
      start,
      Math.min(end, leftCount),
      output,
      count,
      isVisible,
      metrics,
    );
  }
  let ownOffset = leftCount;
  for (let offset = 0; offset < nodeLength(node); offset += 1) {
    const element = nodeElement(node, offset);
    if (!isVisible(element)) continue;
    if (start <= ownOffset && end > ownOffset) output.push(element);
    ownOffset += 1;
    if (ownOffset >= end) break;
  }
  if (end > ownOffset) {
    collectCausalRange(
      node.right,
      Math.max(0, start - ownOffset),
      end - ownOffset,
      output,
      count,
      isVisible,
      metrics,
    );
  }
}

function collectCausalIdRuns<T extends IndexedSequenceElement>(
  node: SequenceNode<T> | undefined,
  start: number,
  end: number,
  output: SequenceIdRun[],
  count: (node: SequenceNode<T> | undefined) => number,
  isVisible: (element: T) => boolean,
): void {
  if (node === undefined || start >= end) return;
  const nodeCount = count(node);
  if (
    start === 0 &&
    end === nodeCount &&
    nodeCount === node.allCount &&
    node.idRunCount === 1
  ) {
    appendSequenceIdRun(output, node.firstId, node.allCount);
    return;
  }
  const leftCount = count(node.left);
  if (start < leftCount) {
    collectCausalIdRuns(
      node.left,
      start,
      Math.min(end, leftCount),
      output,
      count,
      isVisible,
    );
  }
  let ownOffset = leftCount;
  for (let offset = 0; offset < nodeLength(node); offset += 1) {
    const element = nodeElement(node, offset);
    if (!isVisible(element)) continue;
    if (start <= ownOffset && end > ownOffset) {
      appendSequenceIdRun(output, element.id, 1);
    }
    ownOffset += 1;
    if (ownOffset >= end) break;
  }
  if (end > ownOffset) {
    collectCausalIdRuns(
      node.right,
      Math.max(0, start - ownOffset),
      end - ownOffset,
      output,
      count,
      isVisible,
    );
  }
}

function appendSequenceIdRun(
  output: SequenceIdRun[],
  start: SequenceId,
  length: number,
): void {
  if (length <= 0) return;
  const previous = output.at(-1);
  if (
    previous !== undefined &&
    previous.start.peer === start.peer &&
    previous.start.counter + previous.length === start.counter
  ) {
    output[output.length - 1] = {
      start: previous.start,
      length: previous.length + length,
    };
  } else {
    output.push({ start: { ...start }, length });
  }
}

function firstPhysicalElement<T extends IndexedSequenceElement>(
  node: SequenceNode<T> | undefined,
): T | undefined {
  if (node === undefined) return undefined;
  while (node.left !== undefined) node = node.left;
  return nodeElement(node, 0);
}

function nextPhysicalElement<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
  offset: number,
): T | undefined {
  if (offset + 1 < nodeLength(node)) return nodeElement(node, offset + 1);
  const fromRight = firstPhysicalElement(node.right);
  if (fromRight !== undefined) return fromRight;
  let current = node;
  while (current.parent !== undefined) {
    if (current === current.parent.left) return nodeElement(current.parent, 0);
    current = current.parent;
  }
  return undefined;
}

function firstVisibleElement<T extends IndexedSequenceElement>(
  node: SequenceNode<T> | undefined,
  metrics: (element: T) => SequenceMetrics,
): T | undefined {
  while (node !== undefined && node.visibleCount > 0) {
    pushNodeDeletion(node, metrics);
    if (visibleCount(node.left) > 0) {
      node = node.left;
      continue;
    }
    for (let offset = 0; offset < nodeLength(node); offset += 1) {
      const element = nodeElement(node, offset);
      if (!element.deleted) return element;
    }
    node = node.right;
  }
  return undefined;
}

function lastVisibleElement<T extends IndexedSequenceElement>(
  node: SequenceNode<T> | undefined,
  metrics: (element: T) => SequenceMetrics,
): T | undefined {
  while (node !== undefined && node.visibleCount > 0) {
    pushNodeDeletion(node, metrics);
    if (visibleCount(node.right) > 0) {
      node = node.right;
      continue;
    }
    for (let offset = nodeLength(node) - 1; offset >= 0; offset -= 1) {
      const element = nodeElement(node, offset);
      if (!element.deleted) return element;
    }
    node = node.left;
  }
  return undefined;
}

function rightmost<T extends IndexedSequenceElement>(
  node: SequenceNode<T>,
): SequenceNode<T> {
  while (node.right !== undefined) node = node.right;
  return node;
}

function versionsEqual(
  left: ReadonlyMap<bigint, number> | undefined,
  right: ReadonlyMap<bigint, number>,
): boolean {
  if (left === undefined || left.size !== right.size) return false;
  for (const [peer, counter] of left) {
    if (right.get(peer) !== counter) return false;
  }
  return true;
}

interface CounterRange {
  start: number;
  end: number;
}

const MAX_COUNTER_DENSE_GAP = 4_096;
const COUNTER_PAGE_SIZE = 1_024;

class PagedCounterStore<T> {
  readonly #pages = new Map<number, (T | undefined)[]>();
  readonly #pageIndexes: number[] = [];

  set(counter: number, value: T): void {
    const pageIndex = Math.floor(counter / COUNTER_PAGE_SIZE);
    let page = this.#pages.get(pageIndex);
    if (page === undefined) {
      page = [];
      this.#pages.set(pageIndex, page);
      const index = lowerBoundNumber(this.#pageIndexes, pageIndex);
      this.#pageIndexes.splice(index, 0, pageIndex);
    }
    page[counter - pageIndex * COUNTER_PAGE_SIZE] = value;
  }

  some(start: number, end: number, predicate: (value: T) => boolean): boolean {
    if (start >= end) return false;
    const firstPage = Math.floor(start / COUNTER_PAGE_SIZE);
    const lastPage = Math.floor((end - 1) / COUNTER_PAGE_SIZE);
    for (
      let index = lowerBoundNumber(this.#pageIndexes, firstPage);
      index < this.#pageIndexes.length && this.#pageIndexes[index]! <= lastPage;
      index += 1
    ) {
      const pageIndex = this.#pageIndexes[index]!;
      const page = this.#pages.get(pageIndex)!;
      const pageStart = pageIndex * COUNTER_PAGE_SIZE;
      const offsetStart = Math.max(0, start - pageStart);
      const offsetEnd = Math.min(page.length, end - pageStart);
      for (let offset = offsetStart; offset < offsetEnd; offset += 1) {
        const value = page[offset];
        if (value !== undefined && predicate(value)) return true;
      }
    }
    return false;
  }
}

class CounterStore<T> {
  readonly #dense: (T | undefined)[] = [];
  readonly #sparse = new Map<number, T>();
  readonly #sparseCounters = new CounterIndex();

  get(counter: number): T | undefined {
    return counter < this.#dense.length
      ? this.#dense[counter]
      : this.#sparse.get(counter);
  }

  has(counter: number): boolean {
    return this.get(counter) !== undefined;
  }

  set(counter: number, value: T): void {
    if (counter < 0xffff_ffff && counter <= this.#dense.length + MAX_COUNTER_DENSE_GAP) {
      this.#dense[counter] = value;
      return;
    }
    if (!this.#sparse.has(counter)) this.#sparseCounters.add(counter);
    this.#sparse.set(counter, value);
  }

  reserveDense(endExclusive: number): void {
    if (this.#sparse.size > 0) {
      throw new Error("cannot reserve dense sequence counters after sparse insertion");
    }
    if (endExclusive > this.#dense.length) this.#dense.length = endExclusive;
  }

  forEach(start: number, end: number, visit: (value: T, counter: number) => void): void {
    if (start >= end) return;
    const denseEnd = Math.min(end, this.#dense.length);
    for (let counter = Math.max(0, start); counter < denseEnd; counter += 1) {
      const value = this.#dense[counter];
      if (value !== undefined) visit(value, counter);
    }
    this.#sparseCounters.forEach(start, end, (counter) => {
      visit(this.#sparse.get(counter)!, counter);
    });
  }
}

class CounterIndex {
  readonly #ranges: number[] = [];
  readonly #singletons: number[] = [];
  readonly #outOfOrder = new CounterRangeTree();
  #lastCounter: number | undefined;
  #tailIsRange = false;

  add(counter: number): void {
    const last = this.#lastCounter;
    if (last === undefined) {
      this.#singletons.push(counter);
      this.#lastCounter = counter;
      return;
    }
    if (counter > last) {
      if (counter === last + 1) {
        if (this.#tailIsRange) {
          this.#ranges[this.#ranges.length - 1] = counter + 1;
        } else {
          this.#ranges.push(this.#singletons.pop()!, counter + 1);
          this.#tailIsRange = true;
        }
      } else {
        this.#singletons.push(counter);
        this.#tailIsRange = false;
      }
      this.#lastCounter = counter;
      return;
    }
    if (counter === last || containsCounter(this.#ranges, this.#singletons, counter)) {
      return;
    }
    this.#outOfOrder.add(counter);
  }

  forEach(start: number, end: number, visit: (counter: number) => void): void {
    if (start >= end) return;
    const index = lowerBoundCounterRange(this.#ranges, start);
    const previousEnd = this.#ranges[index * 2 - 1];
    if (previousEnd !== undefined && previousEnd > start) {
      visitCounterInterval(
        this.#ranges[(index - 1) * 2]!,
        previousEnd,
        start,
        end,
        visit,
      );
    }
    for (
      let rangeIndex = index;
      rangeIndex < this.#ranges.length / 2 && this.#ranges[rangeIndex * 2]! < end;
      rangeIndex += 1
    ) {
      visitCounterInterval(
        this.#ranges[rangeIndex * 2]!,
        this.#ranges[rangeIndex * 2 + 1]!,
        start,
        end,
        visit,
      );
    }
    for (
      let singletonIndex = lowerBoundNumber(this.#singletons, start);
      singletonIndex < this.#singletons.length && this.#singletons[singletonIndex]! < end;
      singletonIndex += 1
    ) {
      visit(this.#singletons[singletonIndex]!);
    }
    this.#outOfOrder.forEach(start, end, visit);
  }
}

class CounterRangeTree {
  readonly #ranges = new OrderedIndex<CounterRange>(
    (left, right) => left.start - right.start,
  );

  add(counter: number): void {
    const range = { start: counter, end: counter + 1 };
    const index = this.#ranges.lowerBound(range);
    const previous = this.#ranges.at(index - 1);
    if (previous !== undefined && counter < previous.end) return;
    const next = this.#ranges.at(index);
    if (next?.start === counter) return;

    const extendsPrevious = previous?.end === counter;
    const extendsNext = next?.start === counter + 1;
    if (extendsPrevious && extendsNext) {
      previous.end = next.end;
      this.#ranges.delete(next);
    } else if (extendsPrevious) {
      previous.end += 1;
    } else if (extendsNext) {
      this.#ranges.delete(next);
      next.start = counter;
      this.#ranges.add(next);
    } else {
      this.#ranges.add(range);
    }
  }

  forEach(start: number, end: number, visit: (counter: number) => void): void {
    if (start >= end) return;
    const lower = { start, end: start + 1 };
    const index = this.#ranges.lowerBound(lower);
    const previous = this.#ranges.at(index - 1);
    if (previous !== undefined && previous.end > start) {
      visitCounterRange(previous, start, end, visit);
    }
    this.#ranges.forEachFrom(lower, (range) => {
      if (range.start >= end) return false;
      visitCounterRange(range, start, end, visit);
    });
  }
}

function lowerBoundCounterRange(ranges: readonly number[], counter: number): number {
  let low = 0;
  let high = ranges.length / 2;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (ranges[middle * 2]! < counter) low = middle + 1;
    else high = middle;
  }
  return low;
}

function lowerBoundNumber(values: readonly number[], value: number): number {
  let low = 0;
  let high = values.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (values[middle]! < value) low = middle + 1;
    else high = middle;
  }
  return low;
}

function containsCounter(
  ranges: readonly number[],
  singletons: readonly number[],
  counter: number,
): boolean {
  const singletonIndex = lowerBoundNumber(singletons, counter);
  if (singletons[singletonIndex] === counter) return true;
  const rangeIndex = lowerBoundCounterRange(ranges, counter);
  if (ranges[rangeIndex * 2] === counter) return true;
  return rangeIndex > 0 && counter < ranges[rangeIndex * 2 - 1]!;
}

function visitCounterInterval(
  rangeStart: number,
  rangeEnd: number,
  start: number,
  end: number,
  visit: (counter: number) => void,
): void {
  const boundedEnd = Math.min(rangeEnd, end);
  for (let counter = Math.max(rangeStart, start); counter < boundedEnd; counter += 1) {
    visit(counter);
  }
}

function visitCounterRange(
  range: CounterRange,
  start: number,
  end: number,
  visit: (counter: number) => void,
): void {
  const rangeEnd = Math.min(range.end, end);
  for (let counter = Math.max(range.start, start); counter < rangeEnd; counter += 1) {
    visit(counter);
  }
}
