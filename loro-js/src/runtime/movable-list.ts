import type {
  ListStateItemId,
  MovableListStateItem,
  MovableListStateLamportId,
  MovableListStateSnapshot,
} from "../codec/state-snapshot";
import { ContainerType as CodecContainerType } from "../codec/types";
import type { EncodedLoroValue, Id as CodecId } from "../codec/types";
import {
  cloneRuntimeValue,
  insertFugueElements,
  LoroContainer,
  type CausalVersion,
  type LastWriter,
  type RuntimeValue,
  type SequenceElement,
} from "./containers";
import { SequenceIndex, type SequenceMetrics } from "./sequence-index";

/**
 * MovableList state with Rust's model: a Fugue sequence of positions plus
 * elements whose position and value are separate last-writer-wins registers.
 * See context/loro-js-movable-list.md.
 */

/** A list item. `deleted` means it is not alive; see `MovableListState`. */
export interface MovablePosition extends SequenceElement {
  /**
   * The element this position was created for. Undefined for an unpointed
   * position read from a snapshot, whose element is not recorded.
   */
  element: MovableElement | undefined;
  /** Alive at the tracker version minus alive now; see `MovableListState`. */
  trackerDelta?: number | undefined;
}

export interface MovableValueCandidate {
  readonly writer: LastWriter;
  readonly value: RuntimeValue;
  /** The writing op, or undefined for a shallow-root seed that every version includes. */
  readonly id: CodecId | undefined;
}

export interface MovableElement {
  readonly peer: bigint;
  readonly lamport: number;
  value: RuntimeValue;
  valueWriter: LastWriter;
  /** The winning position. The element is visible iff this position is alive. */
  pos: MovablePosition;
  /** Position candidates by `(lamport, peer)`; undefined when history is unknown. */
  positions: MovablePosition[] | undefined;
  /** Value candidates by writer; undefined when history is unknown. */
  values: MovableValueCandidate[] | undefined;
}

export interface MovableElementId {
  readonly peer: bigint;
  readonly lamport: number;
}

/** Receives user-visible changes in the order they happen. */
export interface MovableListEvents {
  delete(position: number, length: number): void;
  insertList(position: number, values: readonly unknown[]): void;
}

export type MovableTransitionOp =
  | {
      readonly type: "insert";
      readonly id: CodecId;
      readonly lamport: number;
      readonly length: number;
    }
  | { readonly type: "delete"; readonly id: CodecId; readonly length: number }
  | { readonly type: "move"; readonly id: CodecId; readonly elementId: MovableElementId }
  | { readonly type: "set"; readonly id: CodecId; readonly elementId: MovableElementId };

export class InvalidMovableListOpError extends Error {}

/** Indexed by pointedness, then by tracker delta + 1. */
const POSITION_METRICS: readonly (readonly SequenceMetrics[])[] = [0, 1].map((utf16) =>
  [-1, 0, 1].map((utf8) => ({ utf16, utf8 })),
);

/**
 * A position counts as one user-visible item (`utf16`) iff its element points
 * at it. `utf8` holds its tracker delta.
 */
function positionMetrics(position: MovablePosition): SequenceMetrics {
  const pointed = position.element !== undefined && position.element.pos === position;
  return POSITION_METRICS[pointed ? 1 : 0]![(position.trackerDelta ?? 0) + 1]!;
}

export function compareLamportIds(
  left: { readonly lamport: number; readonly peer: bigint },
  right: { readonly lamport: number; readonly peer: bigint },
): number {
  return (
    left.lamport - right.lamport ||
    (left.peer < right.peer ? -1 : left.peer > right.peer ? 1 : 0)
  );
}

function positionWriter(position: MovablePosition): LastWriter {
  return { peer: position.id.peer, lamport: position.lamport };
}

function insertSorted<T>(
  items: T[],
  item: T,
  compare: (left: T, right: T) => number,
): void {
  let low = 0;
  let high = items.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (compare(items[middle]!, item) <= 0) low = middle + 1;
    else high = middle;
  }
  items.splice(low, 0, item);
}

function latestIncluded<T>(
  items: readonly T[],
  included: (item: T) => boolean,
): T | undefined {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    if (included(items[index]!)) return items[index];
  }
  return undefined;
}

export function runtimeValuesEqual(left: RuntimeValue, right: RuntimeValue): boolean {
  if (left === right) return true;
  if (left instanceof LoroContainer || right instanceof LoroContainer) return false;
  if (left instanceof Uint8Array || right instanceof Uint8Array) {
    if (!(left instanceof Uint8Array) || !(right instanceof Uint8Array)) return false;
    return (
      left.length === right.length && left.every((byte, index) => byte === right[index])
    );
  }
  if (Array.isArray(left) || Array.isArray(right)) {
    if (!Array.isArray(left) || !Array.isArray(right) || left.length !== right.length) {
      return false;
    }
    return left.every((item, index) => runtimeValuesEqual(item, right[index]!));
  }
  if (
    typeof left === "object" &&
    left !== null &&
    typeof right === "object" &&
    right !== null
  ) {
    const leftKeys = Object.keys(left);
    const rightRecord = right as Record<string, RuntimeValue>;
    return (
      leftKeys.length === Object.keys(right).length &&
      leftKeys.every(
        (key) =>
          Object.prototype.hasOwnProperty.call(right, key) &&
          runtimeValuesEqual(
            (left as Record<string, RuntimeValue>)[key]!,
            rightRecord[key]!,
          ),
      )
    );
  }
  return false;
}

export class MovableListState {
  readonly positions = new SequenceIndex<MovablePosition>(positionMetrics);
  readonly #elements = new Map<bigint, Map<number, MovableElement>>();
  /**
   * False after hydrating latest state from a snapshot: tombstones, origins and
   * candidate history are then unknown, so version transitions and imports that
   * are concurrent with that state must replay history first.
   */
  historyComplete = true;
  /**
   * Rust's tracker (`Tracker::checkout` in the MovableList diff calculator).
   * An op's indices count the positions alive at its causal version. Instead
   * of building that view per op, the tracker keeps a version and gives each
   * position a delta, (alive there) - (alive now), as its `utf8` metric, so
   * `positions.atTracked`/`trackedIndexOf` resolve indices in O(log n).
   * Moving it to another version revisits only the positions that the ops in
   * between created or deleted, and each applied op moves it past itself.
   * Undefined when the tracker is at the current state (all deltas zero).
   */
  #trackerVersion: Map<bigint, number> | undefined;
  readonly #trackedPositions = new Set<MovablePosition>();

  constructor(readonly bindValue: (element: MovableElement) => void) {}

  /** User-visible length. */
  get length(): number {
    return this.positions.visibleUtf16Length;
  }

  /** Length in op indices: alive positions, pointed or not. */
  get opLength(): number {
    return this.positions.visibleLength;
  }

  reset(): void {
    this.#trackerVersion = undefined;
    this.#trackedPositions.clear();
    this.positions.reset();
    this.#elements.clear();
    this.historyComplete = true;
  }

  /**
   * `element`'s value at the version `includes` describes, or `undefined` when
   * the element is not visible there. A state hydrated from a snapshot only
   * knows versions from the snapshot's on (`historyComplete`).
   */
  visibleValueAt(
    element: MovableElement,
    includes: (id: CodecId) => boolean,
  ): { readonly value: RuntimeValue } | undefined {
    const position = latestIncluded(element.positions ?? [element.pos], (candidate) =>
      includes(candidate.id),
    );
    if (position === undefined || this.positions.someDeletion(position, includes)) {
      return undefined;
    }
    const value =
      element.values === undefined
        ? { value: element.value }
        : latestIncluded(
            element.values,
            (candidate) => candidate.id === undefined || includes(candidate.id),
          );
    return value === undefined ? undefined : { value: value.value };
  }

  /** Every element, visible or not. */
  *allElements(): IterableIterator<MovableElement> {
    for (const byLamport of this.#elements.values()) yield* byLamport.values();
  }

  element(peer: bigint, lamport: number): MovableElement | undefined {
    return this.#elements.get(peer)?.get(lamport);
  }

  /** The position of the user-visible item at `index`. */
  positionAt(index: number): MovablePosition | undefined {
    return this.positions.visibleAtMetricOffset(index, "utf16");
  }

  elementAt(index: number): MovableElement | undefined {
    return this.positionAt(index)?.element;
  }

  isVisible(element: MovableElement): boolean {
    return !element.pos.deleted;
  }

  /** User index of a visible element. */
  indexOf(element: MovableElement): number | undefined {
    return this.isVisible(element) ? this.userIndexOf(element.pos) : undefined;
  }

  /** Number of user-visible items before `position`, alive or not. */
  userIndexOf(position: MovablePosition): number {
    return this.positions.visibleMetricOffsetOf(position, "utf16") ?? 0;
  }

  /** Op index of an alive position. */
  opIndexOf(position: MovablePosition): number {
    const index = this.positions.visibleIndexOf(position);
    if (index === undefined || position.deleted) {
      throw new Error("movable-list position is not alive");
    }
    return index;
  }

  /** Op index for a local insertion before the user item at `index`. */
  opIndexForInsert(index: number): number {
    return index >= this.length ? this.opLength : this.opIndexOf(this.positionAt(index)!);
  }

  visibleElements(): MovableElement[] {
    const output: MovableElement[] = [];
    this.positions.forEachVisible((position) => {
      const element = position.element;
      if (element !== undefined && element.pos === position) output.push(element);
    });
    return output;
  }

  visibleElementsRange(start: number, end: number): MovableElement[] {
    const output: MovableElement[] = [];
    for (let index = start; index < end; index += 1) {
      const element = this.elementAt(index);
      if (element !== undefined) output.push(element);
    }
    return output;
  }

  applyInsert(
    opIndex: number,
    values: readonly RuntimeValue[],
    firstId: CodecId,
    lamport: number,
    causalVersion: CausalVersion,
    events?: MovableListEvents,
  ): void {
    if (values.length === 0) return;
    const tracked = this.#track(causalVersion);
    const viewLength = tracked
      ? this.positions.trackedLength
      : this.positions.visibleLength;
    if (!Number.isSafeInteger(opIndex) || opIndex < 0 || opIndex > viewLength) {
      throw new InvalidMovableListOpError(
        `movable-list insert position ${opIndex} is out of range (length ${viewLength})`,
      );
    }
    const inserted: MovablePosition[] = [];
    for (let offset = 0; offset < values.length; offset += 1) {
      const id = { peer: firstId.peer, counter: firstId.counter + offset };
      const writer = { peer: firstId.peer, lamport: lamport + offset };
      const position: MovablePosition = {
        id,
        lamport: writer.lamport,
        value: null,
        deleted: false,
        originLeft: undefined,
        originRight: undefined,
        element: undefined,
      };
      const value = values[offset]!;
      const element: MovableElement = {
        peer: writer.peer,
        lamport: writer.lamport,
        value,
        valueWriter: writer,
        pos: position,
        positions: [position],
        values: [{ writer, value, id }],
      };
      position.element = element;
      this.#registerElement(element);
      inserted.push(position);
    }
    // The origin index misorders a sibling subtree followed by a concurrent
    // element that is not its descendant, so positions use the scan.
    insertFugueElements(
      this.positions,
      opIndex,
      inserted,
      causalVersion,
      false,
      tracked ? this.#trackedLeft(opIndex) : undefined,
    );
    if (tracked) this.#trackPast(firstId, values.length);
    for (const position of inserted) this.bindValue(position.element!);
    if (events !== undefined) {
      events.insertList(
        this.userIndexOf(inserted[0]!),
        values.map((value) => cloneRuntimeValue(value)),
      );
    }
  }

  /** Deletes the positions `startId..startId+|length|` (op `deletedBy` onwards). */
  applyDelete(
    startId: CodecId,
    length: number,
    deletedBy: CodecId,
    events?: MovableListEvents,
  ): void {
    const count = Math.abs(length);
    for (let offset = 0; offset < count; offset += 1) {
      const position = this.positions.findById({
        peer: startId.peer,
        counter: startId.counter + offset,
      });
      if (position === undefined) continue;
      // In a reversed span the k-th atom deletes the k-th item from the end.
      const byOffset = length < 0 ? count - 1 - offset : offset;
      this.#deletePosition(
        position,
        { peer: deletedBy.peer, counter: deletedBy.counter + byOffset },
        events,
      );
    }
    if (this.#trackerVersion !== undefined) this.#trackPast(deletedBy, count);
  }

  /**
   * Rust's tracker deletes the item at `from` in the op's causal view, then
   * inserts the new position at `to` in the view after that delete.
   */
  applyMove(
    from: number,
    to: number,
    elementId: MovableElementId,
    opId: CodecId,
    lamport: number,
    causalVersion: CausalVersion,
    events?: MovableListEvents,
  ): void {
    const element = this.element(elementId.peer, elementId.lamport);
    if (element === undefined) {
      throw new InvalidMovableListOpError(
        `movable-list move targets unknown element L${elementId.lamport}@${elementId.peer}`,
      );
    }
    const tracked = this.#track(causalVersion);
    const viewLength = tracked
      ? this.positions.trackedLength
      : this.positions.visibleLength;
    const source = tracked
      ? this.positions.atTracked(from)
      : this.positions.atVisible(from);
    if (source === undefined) {
      throw new InvalidMovableListOpError(
        `movable-list move source ${from} is out of range (length ${viewLength})`,
      );
    }
    // `to` indexes the view after the source is deleted.
    if (!Number.isSafeInteger(to) || to < 0 || to > viewLength - 1) {
      throw new InvalidMovableListOpError(
        `movable-list move destination ${to} is out of range (length ${viewLength - 1})`,
      );
    }
    this.#deletePosition(source, opId, events);
    // `to` counts the view after the source is deleted.
    if (tracked) this.#setTrackedAlive(source, false);
    const afterDelete = new Map(causalVersion);
    afterDelete.set(
      opId.peer,
      Math.max(afterDelete.get(opId.peer) ?? 0, opId.counter + 1),
    );
    const position: MovablePosition = {
      id: { ...opId },
      lamport,
      value: null,
      deleted: false,
      originLeft: undefined,
      originRight: undefined,
      element,
    };
    insertFugueElements(
      this.positions,
      to,
      [position],
      afterDelete,
      false,
      tracked ? this.#trackedLeft(to) : undefined,
    );
    if (tracked) this.#trackPast(opId, 1);
    if (element.positions !== undefined) {
      insertSorted(element.positions, position, (left, right) =>
        compareLamportIds(positionWriter(left), positionWriter(right)),
      );
    }
    if (compareLamportIds(positionWriter(position), positionWriter(element.pos)) > 0) {
      this.#setWinnerPosition(element, position, events);
    }
  }

  applySet(
    elementId: MovableElementId,
    value: RuntimeValue,
    writer: LastWriter,
    opId: CodecId | undefined,
    events?: MovableListEvents,
    local = false,
  ): void {
    const element = this.element(elementId.peer, elementId.lamport);
    if (element === undefined) {
      throw new InvalidMovableListOpError(
        `movable-list set targets unknown element L${elementId.lamport}@${elementId.peer}`,
      );
    }
    const candidate = { writer, value, id: opId === undefined ? undefined : { ...opId } };
    if (element.values !== undefined) {
      insertSorted(element.values, candidate, (left, right) =>
        compareLamportIds(left.writer, right.writer),
      );
    }
    if (compareLamportIds(writer, element.valueWriter) <= 0) return;
    this.#setWinnerValue(element, candidate, events, local);
  }

  /**
   * Moves the state to the version selected by `includes` after the given ops
   * were retreated or forwarded. Only the positions and elements those ops
   * touch are revisited.
   */
  transition(
    ops: readonly MovableTransitionOp[],
    includes: (id: CodecId) => boolean,
    events?: MovableListEvents,
  ): void {
    this.#clearTracker();
    const positions = new Set<MovablePosition>();
    const elements = new Set<MovableElement>();
    const addPosition = (position: MovablePosition | undefined): void => {
      if (position === undefined) return;
      positions.add(position);
      if (position.element !== undefined) elements.add(position.element);
    };
    for (const op of ops) {
      switch (op.type) {
        case "insert":
          for (let offset = 0; offset < op.length; offset += 1) {
            addPosition(
              this.positions.findById({
                peer: op.id.peer,
                counter: op.id.counter + offset,
              }),
            );
          }
          break;
        case "delete":
          for (const position of this.positions.elementsDeletedBy(
            op.id.peer,
            op.id.counter,
            op.id.counter + op.length,
          )) {
            addPosition(position);
          }
          break;
        case "move":
          addPosition(this.positions.findById(op.id));
          for (const position of this.positions.elementsDeletedBy(
            op.id.peer,
            op.id.counter,
            op.id.counter + 1,
          )) {
            addPosition(position);
          }
          break;
        case "set": {
          const element = this.element(op.elementId.peer, op.elementId.lamport);
          if (element !== undefined) elements.add(element);
          break;
        }
      }
    }
    for (const element of elements) positions.add(element.pos);

    const before = new Map<MovablePosition, { index: number; value: RuntimeValue }>();
    if (events !== undefined) {
      for (const position of positions) {
        const element = position.element;
        if (!position.deleted && element !== undefined && element.pos === position) {
          before.set(position, {
            index: this.userIndexOf(position),
            value: element.value,
          });
        }
      }
    }

    for (const position of positions) {
      const alive =
        includes(position.id) && !this.positions.someDeletion(position, includes);
      if (alive === position.deleted) this.positions.setDeleted(position, !alive);
    }
    for (const element of elements) {
      const candidates = element.positions;
      const values = element.values;
      if (candidates === undefined || values === undefined) {
        throw new Error("movable-list transition needs complete history");
      }
      const winner =
        latestIncluded(candidates, (position) => includes(position.id)) ?? candidates[0]!;
      if (winner !== element.pos) {
        const previous = element.pos;
        element.pos = winner;
        this.positions.refreshMetrics(previous);
        this.positions.refreshMetrics(winner);
        positions.add(winner);
      }
      const value =
        latestIncluded(
          values,
          (candidate) => candidate.id === undefined || includes(candidate.id),
        ) ?? values[0]!;
      if (
        value.value !== element.value ||
        compareLamportIds(value.writer, element.valueWriter) !== 0
      ) {
        element.value = value.value;
        element.valueWriter = value.writer;
        this.bindValue(element);
      }
    }

    if (events === undefined) return;
    const after = new Map<MovablePosition, { index: number; value: RuntimeValue }>();
    for (const position of positions) {
      const element = position.element;
      if (!position.deleted && element !== undefined && element.pos === position) {
        after.set(position, { index: this.userIndexOf(position), value: element.value });
      }
    }
    const removed = [...before]
      .filter(([position, state]) => {
        const next = after.get(position);
        return next === undefined || !runtimeValuesEqual(next.value, state.value);
      })
      .map(([, state]) => state.index)
      .sort((left, right) => right - left);
    for (const index of removed) events.delete(index, 1);
    const inserted = [...after]
      .filter(([position, state]) => {
        const previous = before.get(position);
        return previous === undefined || !runtimeValuesEqual(previous.value, state.value);
      })
      .map(([, state]) => state)
      .sort((left, right) => left.index - right.index);
    for (const state of inserted) {
      events.insertList(state.index, [cloneRuntimeValue(state.value)]);
    }
  }

  /** Whether version transitions over `ops` can run without replaying history. */
  canTransition(ops: readonly MovableTransitionOp[]): boolean {
    if (!this.historyComplete) return false;
    for (const op of ops) {
      switch (op.type) {
        case "insert":
          if (!this.positions.containsIdRuns([{ start: op.id, length: op.length }])) {
            return false;
          }
          break;
        case "delete": {
          let recorded = 0;
          for (const run of this.positions.idRunsDeletedBy(
            op.id.peer,
            op.id.counter,
            op.id.counter + op.length,
          )) {
            recorded += run.length;
          }
          if (recorded < op.length) return false;
          break;
        }
        case "move":
          if (this.positions.findById(op.id) === undefined) return false;
          if (
            this.positions.elementsDeletedBy(op.id.peer, op.id.counter, op.id.counter + 1)
              .length === 0
          ) {
            return false;
          }
          break;
        case "set": {
          const element = this.element(op.elementId.peer, op.elementId.lamport);
          if (
            element?.values === undefined ||
            !element.values.some(
              (candidate) =>
                candidate.id !== undefined &&
                candidate.id.peer === op.id.peer &&
                candidate.id.counter === op.id.counter,
            )
          ) {
            return false;
          }
          break;
        }
      }
    }
    return true;
  }

  /** Encodes the state in Rust's `MovableListState` snapshot layout. */
  encodeSnapshot(
    encodeValue: (value: RuntimeValue) => EncodedLoroValue,
  ): MovableListStateSnapshot {
    const peers: bigint[] = [];
    const peerIndices = new Map<bigint, number>();
    const peerIndex = (peer: bigint): bigint => {
      let index = peerIndices.get(peer);
      if (index === undefined) {
        index = peers.length;
        peers.push(peer);
        peerIndices.set(peer, index);
      }
      return BigInt(index);
    };
    const values: EncodedLoroValue[] = [];
    const items: {
      -readonly [Key in keyof MovableListStateItem]: MovableListStateItem[Key];
    }[] = [
      {
        invisibleListItems: 0n,
        positionIdEqualsElementId: true,
        elementIdEqualsLastSetId: true,
      },
    ];
    const listItemIds: ListStateItemId[] = [];
    const elementIds: MovableListStateLamportId[] = [];
    const lastSetIds: MovableListStateLamportId[] = [];
    this.positions.forEachVisible((position) => {
      const element = position.element;
      const listItemId = {
        peerIndex: peerIndex(position.id.peer),
        counter: position.id.counter,
        lamportSub: position.lamport - position.id.counter,
      };
      if (element === undefined || element.pos !== position) {
        items.at(-1)!.invisibleListItems += 1n;
        listItemIds.push(listItemId);
        return;
      }
      const positionIdEqualsElementId =
        position.id.peer === element.peer && position.lamport === element.lamport;
      const elementIdEqualsLastSetId =
        element.valueWriter.peer === element.peer &&
        element.valueWriter.lamport === element.lamport;
      items.push({
        invisibleListItems: 0n,
        positionIdEqualsElementId,
        elementIdEqualsLastSetId,
      });
      listItemIds.push(listItemId);
      if (!positionIdEqualsElementId) {
        elementIds.push({ peerIndex: peerIndex(element.peer), lamport: element.lamport });
      }
      if (!elementIdEqualsLastSetId) {
        lastSetIds.push({
          peerIndex: peerIndex(element.valueWriter.peer),
          lamport: element.valueWriter.lamport,
        });
      }
      values.push(encodeValue(element.value));
    });
    return {
      kind: CodecContainerType.MovableList,
      values,
      peers,
      items,
      listItemIds,
      elementIds,
      lastSetIds,
    };
  }

  /**
   * Hydrates Rust's snapshot layout. A shallow root seeds candidate history,
   * because no version before the root can be checked out; latest state does
   * not, so the state is marked incomplete.
   */
  hydrate(
    state: MovableListStateSnapshot,
    decodeValue: (value: EncodedLoroValue) => RuntimeValue,
    shallowRoot: boolean,
  ): void {
    const positions: MovablePosition[] = [];
    const listItemIds = state.listItemIds;
    let listIndex = 0;
    let elementIndex = 0;
    let lastSetIndex = 0;
    const peer = (index: bigint): bigint => {
      const value = state.peers[Number(index)];
      if (value === undefined)
        throw new Error("movable-list state peer index out of range");
      return value;
    };
    const nextPosition = (): MovablePosition => {
      const item = listItemIds[listIndex++];
      if (item === undefined)
        throw new Error("movable-list state is missing a list item ID");
      return {
        id: { peer: peer(item.peerIndex), counter: item.counter },
        lamport: item.counter + item.lamportSub,
        value: null,
        deleted: false,
        originLeft: undefined,
        originRight: undefined,
        element: undefined,
      };
    };
    for (let itemIndex = 0; itemIndex < state.items.length; itemIndex += 1) {
      const item = state.items[itemIndex]!;
      if (itemIndex > 0) {
        const position = nextPosition();
        const elementId = item.positionIdEqualsElementId
          ? { peer: position.id.peer, lamport: position.lamport }
          : readLamportId(state.elementIds[elementIndex++], peer);
        const valueWriter = item.elementIdEqualsLastSetId
          ? { ...elementId }
          : readLamportId(state.lastSetIds[lastSetIndex++], peer);
        const encoded = state.values[itemIndex - 1];
        if (encoded === undefined)
          throw new Error("movable-list state is missing a value");
        const value = decodeValue(encoded);
        const element: MovableElement = {
          peer: elementId.peer,
          lamport: elementId.lamport,
          value,
          valueWriter,
          pos: position,
          // Seeds that every later version includes. Without the snapshot's
          // history (`historyComplete`), they are only right from its version on.
          positions: [position],
          values: [{ writer: valueWriter, value, id: undefined }],
        };
        position.element = element;
        this.#registerElement(element);
        positions.push(position);
      }
      for (let index = 0n; index < item.invisibleListItems; index += 1n) {
        positions.push(nextPosition());
      }
    }
    if (positions.length > 0)
      this.positions.insertAtPhysical(this.positions.allLength, positions);
    for (const position of positions) {
      if (position.element !== undefined) this.bindValue(position.element);
    }
    if (!shallowRoot) this.historyComplete = false;
  }

  #registerElement(element: MovableElement): void {
    let byLamport = this.#elements.get(element.peer);
    if (byLamport === undefined) {
      byLamport = new Map();
      this.#elements.set(element.peer, byLamport);
    }
    if (byLamport.has(element.lamport)) {
      throw new Error(
        `duplicate movable-list element L${element.lamport}@${element.peer}`,
      );
    }
    byLamport.set(element.lamport, element);
  }

  /**
   * Moves the tracker to `version`. Returns false, with the tracker cleared,
   * when `version` includes every op applied so far (the current state).
   */
  #track(version: CausalVersion): boolean {
    if (this.positions.isFullyIncluded(version)) {
      this.#clearTracker();
      return false;
    }
    const from = this.#trackerVersion ?? this.positions.idEnds();
    const affected = new Set<MovablePosition>();
    const visit = (position: MovablePosition): void => {
      affected.add(position);
    };
    for (const peer of new Set([...from.keys(), ...version.keys()])) {
      const start = Math.min(from.get(peer) ?? 0, version.get(peer) ?? 0);
      const end = Math.max(from.get(peer) ?? 0, version.get(peer) ?? 0);
      if (start === end) continue;
      this.positions.forEachWithIdIn(peer, start, end, visit);
      for (const position of this.positions.elementsDeletedBy(peer, start, end)) {
        affected.add(position);
      }
    }
    const target = new Map(version);
    this.#trackerVersion = target;
    const includes = (id: CodecId): boolean => id.counter < (target.get(id.peer) ?? 0);
    for (const position of affected) {
      this.#setTrackedAlive(
        position,
        includes(position.id) && !this.positions.someDeletion(position, includes),
      );
    }
    return true;
  }

  /** Moves the tracker past the op `id..id+length` it just applied. */
  #trackPast(id: CodecId, length: number): void {
    const version = this.#trackerVersion;
    if (version === undefined) return;
    const next = new Map(version);
    next.set(id.peer, Math.max(next.get(id.peer) ?? 0, id.counter + length));
    this.#track(next);
  }

  #setTrackedAlive(position: MovablePosition, alive: boolean): void {
    const delta = (alive ? 1 : 0) - (position.deleted ? 0 : 1);
    if (delta === (position.trackerDelta ?? 0)) return;
    position.trackerDelta = delta === 0 ? undefined : delta;
    if (delta === 0) this.#trackedPositions.delete(position);
    else this.#trackedPositions.add(position);
    this.positions.refreshMetrics(position);
  }

  #clearTracker(): void {
    this.#trackerVersion = undefined;
    for (const position of this.#trackedPositions) {
      position.trackerDelta = undefined;
      this.positions.refreshMetrics(position);
    }
    this.#trackedPositions.clear();
  }

  /** Fugue origins for an insertion at tracked index `index`. */
  #trackedLeft(index: number): {
    readonly current: false;
    readonly left: MovablePosition | undefined;
  } {
    return {
      current: false,
      left: index === 0 ? undefined : this.positions.atTracked(index - 1),
    };
  }

  #deletePosition(
    position: MovablePosition,
    deletedBy: CodecId,
    events: MovableListEvents | undefined,
  ): void {
    const element = position.element;
    const hides = !position.deleted && element !== undefined && element.pos === position;
    if (events !== undefined && hides) events.delete(this.userIndexOf(position), 1);
    this.positions.deleteElement(position, deletedBy);
  }

  /**
   * Events follow Rust's `convert_update_to_event_pos`: insert at the new
   * index, then delete the old item. When the new item directly follows the
   * old one the two cancel and no event is emitted.
   */
  #setWinnerPosition(
    element: MovableElement,
    position: MovablePosition,
    events: MovableListEvents | undefined,
  ): void {
    const previous = element.pos;
    element.pos = position;
    this.positions.refreshMetrics(previous);
    this.positions.refreshMetrics(position);
    if (events === undefined) return;
    const removed = previous.deleted ? undefined : this.userIndexOf(previous);
    if (!position.deleted) {
      let inserted = this.userIndexOf(position);
      if (removed !== undefined && inserted > removed) inserted += 1;
      events.insertList(inserted, [cloneRuntimeValue(element.value)]);
    }
    if (removed !== undefined) events.delete(removed, 1);
  }

  #setWinnerValue(
    element: MovableElement,
    candidate: MovableValueCandidate,
    events: MovableListEvents | undefined,
    local: boolean,
  ): void {
    const changed = local || !runtimeValuesEqual(element.value, candidate.value);
    if (events !== undefined && changed && this.isVisible(element)) {
      const index = this.userIndexOf(element.pos);
      events.delete(index, 1);
      events.insertList(index, [cloneRuntimeValue(candidate.value)]);
    }
    element.value = candidate.value;
    element.valueWriter = candidate.writer;
    this.bindValue(element);
  }
}

function readLamportId(
  id: MovableListStateLamportId | undefined,
  peer: (index: bigint) => bigint,
): LastWriter {
  if (id === undefined) throw new Error("movable-list state is missing an element ID");
  return { peer: peer(id.peerIndex), lamport: id.lamport };
}

/**
 * Whether two states hold the same alive positions in the same order, with
 * the same elements, values and last writers: a snapshot state against the
 * replay of its history.
 */
export function sameMovableListStates(
  left: MovableListState,
  right: MovableListState,
): boolean {
  if (left.positions.visibleLength !== right.positions.visibleLength) return false;
  const alive = (state: MovableListState): MovablePosition[] => {
    const positions: MovablePosition[] = [];
    state.positions.forEachVisible((position) => {
      positions.push(position);
    });
    return positions;
  };
  const rightPositions = alive(right);
  return alive(left).every((position, index) => {
    const other = rightPositions[index]!;
    if (position.id.peer !== other.id.peer || position.id.counter !== other.id.counter) {
      return false;
    }
    const element = position.element?.pos === position ? position.element : undefined;
    const otherElement = other.element?.pos === other ? other.element : undefined;
    if (element === undefined || otherElement === undefined) {
      return element === otherElement;
    }
    return (
      element.peer === otherElement.peer &&
      element.lamport === otherElement.lamport &&
      compareLamportIds(element.valueWriter, otherElement.valueWriter) === 0 &&
      runtimeValuesEqual(element.value, otherElement.value)
    );
  });
}
