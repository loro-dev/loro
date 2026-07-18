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
  meta: UndoItemValue;
  timestamp: number;
  targets: Set<string>;
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
  readonly #redo: UndoItem[] = [];
  readonly #excludeOriginPrefixes = new Set<string>();
  readonly #remoteTargets = new Set<string>();
  #mergeInterval: number;
  #maxUndoSteps: number;
  #onPush: UndoConfig["onPush"];
  #onPop: UndoConfig["onPop"];
  #applying = false;
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
    const item = this.#undo.pop();
    if (item === undefined) return false;
    try {
      const redo = this.#invert(item, true);
      if (redo !== undefined) this.#redo.push(redo);
    } catch (error) {
      this.#undo.push(item);
      throw error;
    }
    return true;
  }

  redo(): boolean {
    const item = this.#redo.pop();
    if (item === undefined) return false;
    try {
      const undo = this.#invert(item, false);
      if (undo !== undefined) this.#pushUndo(undo, false);
    } catch (error) {
      this.#redo.push(item);
      throw error;
    }
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
    return this.#redo.at(-1)?.meta.value;
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
    this.#redo.length = 0;
    this.#remoteTargets.clear();
  }

  clearUndo(): void {
    this.#undo.clear();
  }

  clearRedo(): void {
    this.#redo.length = 0;
  }

  destroy(): void {
    this.#unsubscribe();
    this.clear();
  }

  #record(event: LoroEventBatch): void {
    if (this.#applying) return;
    const targets = new Set(event.events.map(({ target }) => target));
    if (event.by === "checkout") {
      this.clear();
      return;
    }
    if (event.by === "import") {
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
    const now = Date.now();
    const item: UndoItem = {
      peer: span.peer,
      range: { start: span.counter, end: span.counter + span.length },
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
    this.#redo.length = 0;
    this.#remoteTargets.clear();
  }

  #invert(item: UndoItem, isUndo: boolean): UndoItem | undefined {
    const before = this.#doc.frontiers();
    this.#applying = true;
    try {
      this.#doc._undoIdSpan(item.peer, item.range);
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
      return {
        peer: span.peer,
        range,
        meta: this.#onPush?.(!isUndo, range) ?? EMPTY_META,
        timestamp: Date.now(),
        targets: new Set(item.targets),
      };
    } finally {
      this.#applying = false;
    }
  }

  #pushUndo(item: UndoItem, clearRedo: boolean): void {
    this.#undo.push(item);
    this.#trimUndo();
    if (clearRedo) this.#redo.length = 0;
  }

  #trimUndo(): void {
    this.#undo.trimFront(this.#maxUndoSteps);
  }
}
