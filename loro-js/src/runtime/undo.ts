import type { LoroDoc } from "./document";
import type {
  CounterSpan,
  LoroEventBatch,
  PeerID,
  UndoConfig,
  UndoItemValue,
} from "./types";

interface UndoItem {
  peer: PeerID;
  range: CounterSpan;
  // Lamport of the first op in `range`.
  lamport: number;
  meta: UndoItemValue;
  timestamp: number;
  targets: Set<string>;
}

/** A counter range and the lamport of its last op. */
interface TrackedRange extends CounterSpan {
  lastLamport: number;
}

class UndoDeque<T> {
  readonly #items = new Map<number, T>();
  #start = 0;
  #end = 0;

  get length(): number {
    return this.#end - this.#start;
  }

  push(item: T): void {
    this.#items.set(this.#end, item);
    this.#end += 1;
  }

  pop(): T | undefined {
    if (this.#end === this.#start) return undefined;
    this.#end -= 1;
    const item = this.#items.get(this.#end);
    this.#items.delete(this.#end);
    if (this.#end === this.#start) this.clear();
    return item;
  }

  peek(): T | undefined {
    return this.#end === this.#start ? undefined : this.#items.get(this.#end - 1);
  }

  /** The oldest item. */
  first(): T | undefined {
    return this.#end === this.#start ? undefined : this.#items.get(this.#start);
  }

  trimFront(length: number): void {
    while (this.length > length) {
      this.#items.delete(this.#start);
      this.#start += 1;
    }
    if (this.#end === this.#start) this.clear();
  }

  clear(): void {
    this.#items.clear();
    this.#start = 0;
    this.#end = 0;
  }
}

const EMPTY_META: UndoItemValue = { value: null, cursors: [] };

export class UndoManager {
  readonly #doc: LoroDoc;
  #peer: PeerID;
  readonly #undo = new UndoDeque<UndoItem>();
  readonly #redo = new UndoDeque<UndoItem>();
  readonly #excludeOriginPrefixes = new Set<string>();
  readonly #remoteTargets = new Set<string>();
  #mergeInterval: number;
  #maxUndoSteps: number;
  #onPush: UndoConfig["onPush"];
  #onPop: UndoConfig["onPop"];
  #applying = false;
  // Every counter range this manager recorded or wrote while undoing or
  // redoing, per peer, merged when adjacent. Later moves inside these do not
  // block undoing an earlier move; any other later move does.
  // Each peer's ranges from `head` on are live; the dropped prefix is compacted
  // away in bulk.
  readonly #tracked = new Map<bigint, { ranges: TrackedRange[]; head: number }>();
  #paused = false;
  #groupDepth = 0;
  #unsubscribe: () => void;

  constructor(doc: LoroDoc, config: UndoConfig = {}) {
    this.#doc = doc;
    this.#peer = doc.peerIdStr;
    this.#mergeInterval = config.mergeInterval ?? 1000;
    this.#maxUndoSteps = config.maxUndoSteps ?? 100;
    this.#onPush = config.onPush;
    this.#onPop = config.onPop;
    for (const prefix of config.excludeOriginPrefixes ?? []) {
      this.#excludeOriginPrefixes.add(prefix);
    }
    this.#unsubscribe = doc.subscribe((event) => this.#record(event));
  }

  free(): void {
    this.destroy();
  }

  undo(): boolean {
    if (this.#paused) return false;
    const item = this.#undo.pop();
    if (item === undefined) return false;
    try {
      const redo = this.#invert(item, true);
      if (redo !== undefined) this.#redo.push(redo);
    } catch (error) {
      this.#undo.push(item);
      throw error;
    }
    this.#pruneTracked();
    return true;
  }

  redo(): boolean {
    if (this.#paused) return false;
    const item = this.#redo.pop();
    if (item === undefined) return false;
    try {
      const undo = this.#invert(item, false);
      if (undo !== undefined) this.#pushUndo(undo, false);
    } catch (error) {
      this.#redo.push(item);
      throw error;
    }
    this.#pruneTracked();
    return true;
  }

  peer(): PeerID {
    return this.#peer;
  }

  groupStart(): void {
    this.#groupDepth += 1;
  }

  groupEnd(): void {
    if (this.#groupDepth > 0) this.#groupDepth -= 1;
  }

  canUndo(): boolean {
    return this.#undo.length > 0;
  }

  canRedo(): boolean {
    return this.#redo.length > 0;
  }

  topUndoValue(): unknown {
    return this.#undo.peek()?.meta.value;
  }

  topRedoValue(): unknown {
    return this.#redo.peek()?.meta.value;
  }

  setMaxUndoSteps(steps: number): void {
    if (!Number.isSafeInteger(steps) || steps < 0) {
      throw new RangeError("max undo steps must be a nonnegative integer");
    }
    this.#maxUndoSteps = steps;
    this.#trimUndo();
  }

  setMergeInterval(interval: number): void {
    if (!Number.isFinite(interval) || interval < 0) {
      throw new RangeError("merge interval must be nonnegative");
    }
    this.#mergeInterval = interval;
  }

  addExcludeOriginPrefix(prefix: string): void {
    this.#excludeOriginPrefixes.add(prefix);
  }

  setOnPush(callback: UndoConfig["onPush"]): void {
    this.#onPush = callback;
  }

  setOnPop(callback: UndoConfig["onPop"]): void {
    this.#onPop = callback;
  }

  clear(): void {
    this.#undo.clear();
    this.#redo.clear();
    this.#remoteTargets.clear();
    this.#tracked.clear();
  }

  clearUndo(): void {
    this.#undo.clear();
    this.#pruneTracked();
  }

  clearRedo(): void {
    this.#redo.clear();
    this.#pruneTracked();
  }

  pause(): void {
    this.#paused = true;
  }

  resume(): void {
    this.#paused = false;
  }

  isPaused(): boolean {
    return this.#paused;
  }

  destroy(): void {
    this.#unsubscribe();
    this.clear();
  }

  #record(event: LoroEventBatch): void {
    if (this.#applying) return;
    const targets = new Set(event.events.map(({ target }) => target));
    if (event.by === "checkout") {
      if (!this.#paused) this.clear();
      return;
    }
    if (event.by === "import") {
      for (const target of targets) this.#remoteTargets.add(target);
      return;
    }
    if (this.#paused) {
      for (const target of targets) this.#remoteTargets.add(target);
      return;
    }
    if (
      event.origin !== undefined &&
      [...this.#excludeOriginPrefixes].some((prefix) => event.origin!.startsWith(prefix))
    ) {
      for (const target of targets) this.#remoteTargets.add(target);
      return;
    }
    const spans = this.#doc.findIdSpansBetween(event.from, event.to).forward;
    const span =
      spans.find(({ peer }) => peer === this.#doc.peerIdStr) ??
      (spans.length === 1 ? spans[0] : undefined);
    if (span === undefined || span.length === 0) return;
    this.#peer = span.peer;
    const range = { start: span.counter, end: span.counter + span.length };
    this.#track(span.peer, range);
    const now = Date.now();
    const item: UndoItem = {
      peer: span.peer,
      range,
      lamport: this.#doc._lamportOf(span.peer, range.start),
      meta:
        this.#onPush?.(
          true,
          { start: span.counter, end: span.counter + span.length },
          event,
        ) ?? EMPTY_META,
      timestamp: now,
      targets,
    };
    const previous = this.#undo.peek();
    const conflictsWithRemote =
      previous !== undefined &&
      [...previous.targets].some((target) => this.#remoteTargets.has(target));
    const merge =
      previous !== undefined &&
      previous.peer === item.peer &&
      previous.range.end === item.range.start &&
      !conflictsWithRemote &&
      (this.#groupDepth > 0 ||
        (this.#mergeInterval > 0 && now - previous.timestamp < this.#mergeInterval));
    if (merge) {
      previous.range = { start: previous.range.start, end: item.range.end };
      previous.meta = item.meta;
      previous.timestamp = now;
      for (const target of item.targets) previous.targets.add(target);
    } else {
      this.#pushUndo(item, false);
    }
    this.#redo.clear();
    this.#remoteTargets.clear();
    this.#pruneTracked();
  }

  #invert(item: UndoItem, isUndo: boolean): UndoItem | undefined {
    const before = this.#doc.frontiers();
    this.#applying = true;
    try {
      this.#doc._undoIdSpan(item.peer, item.range, (id) => this.#isTracked(id));
      this.#doc.commit({ origin: isUndo ? "undo" : "redo" });
      const after = this.#doc.frontiers();
      const spans = this.#doc.findIdSpansBetween(before, after).forward;
      const span =
        spans.find(({ peer }) => peer === this.#doc.peerIdStr) ??
        (spans.length === 1 ? spans[0] : undefined);
      const poppedMeta: UndoItemValue = {
        value: item.meta.value,
        cursors: this.#doc._transformUndoCursors(item.meta.cursors),
      };
      this.#onPop?.(isUndo, poppedMeta, item.range);
      if (span === undefined || span.length === 0) return undefined;
      const range = { start: span.counter, end: span.counter + span.length };
      this.#peer = span.peer;
      this.#track(span.peer, range);
      return {
        peer: span.peer,
        range,
        lamport: this.#doc._lamportOf(span.peer, range.start),
        meta: this.#onPush?.(!isUndo, range) ?? EMPTY_META,
        timestamp: Date.now(),
        targets: new Set(item.targets),
      };
    } finally {
      this.#applying = false;
    }
  }

  #track(peer: PeerID, range: CounterSpan): void {
    const key = BigInt(peer);
    const lastLamport = this.#doc._lamportOf(peer, range.end - 1);
    let tracked = this.#tracked.get(key);
    if (tracked === undefined) {
      tracked = { ranges: [], head: 0 };
      this.#tracked.set(key, tracked);
    }
    const { ranges } = tracked;
    const last = ranges.length > tracked.head ? ranges.at(-1) : undefined;
    if (last !== undefined && last.end === range.start) {
      ranges[ranges.length - 1] = { start: last.start, end: range.end, lastLamport };
    } else {
      ranges.push({ start: range.start, end: range.end, lastLamport });
    }
  }

  /** Number of tracked counter ranges (internal; used by tests). */
  _trackedRangeCount(): number {
    let count = 0;
    for (const { ranges, head } of this.#tracked.values()) count += ranges.length - head;
    return count;
  }

  /**
   * Keeps only the tracked ranges that can still matter. Undoing a move checks
   * only the moves after it, and the undone move is an op of a stacked item,
   * so a range whose ops all precede every stacked op (by lamport, over all
   * peers) is dropped. Each stacked item was written after every item below
   * it, so the earliest stacked op is the first op of the bottom item of one
   * of the stacks. The cost is O(tracked peers + dropped ranges), independent
   * of the stack lengths.
   */
  #pruneTracked(): void {
    const earliest = Math.min(
      this.#undo.first()?.lamport ?? Infinity,
      this.#redo.first()?.lamport ?? Infinity,
    );
    for (const [peer, tracked] of this.#tracked) {
      const { ranges } = tracked;
      while (tracked.head < ranges.length && ranges[tracked.head]!.lastLamport < earliest)
        tracked.head += 1;
      if (tracked.head === ranges.length) {
        this.#tracked.delete(peer);
      } else if (tracked.head >= 32 && tracked.head * 2 >= ranges.length) {
        ranges.splice(0, tracked.head);
        tracked.head = 0;
      }
    }
  }

  #isTracked(id: { readonly peer: bigint; readonly counter: number }): boolean {
    const tracked = this.#tracked.get(id.peer);
    if (tracked === undefined) return false;
    const { ranges, head } = tracked;
    let low = head;
    let high = ranges.length;
    while (low < high) {
      const middle = (low + high) >>> 1;
      if (ranges[middle]!.start <= id.counter) low = middle + 1;
      else high = middle;
    }
    const range = low > head ? ranges[low - 1] : undefined;
    return range !== undefined && id.counter < range.end;
  }

  #pushUndo(item: UndoItem, clearRedo: boolean): void {
    this.#undo.push(item);
    this.#trimUndo();
    if (clearRedo) this.#redo.clear();
  }

  #trimUndo(): void {
    this.#undo.trimFront(this.#maxUndoSteps);
    this.#pruneTracked();
  }
}
