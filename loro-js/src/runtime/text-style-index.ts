import { OrderedIndex } from "./ordered-index";
import type { SequenceId, SequenceIdRun } from "./sequence-index";

export interface IndexedTextStyleMeta<Value = unknown> {
  readonly startId: SequenceId;
  readonly lamport: number;
  readonly info: number;
  readonly value: Value;
}

export interface TextStyleTransition<Meta> {
  readonly run: SequenceIdRun;
  readonly before: Meta | undefined;
  readonly after: Meta | undefined;
}

interface StyleSegment<Meta> {
  start: number;
  end: number;
  readonly histories: Map<string, Meta[]>;
  latestMetas: ReadonlyMap<string, Meta> | undefined;
}

interface SegmentLookup<Meta> {
  readonly start: number;
  readonly end: number;
  readonly segment: StyleSegment<Meta> | undefined;
}

export class TextStyleIndex<Meta extends IndexedTextStyleMeta> {
  readonly #segmentsByPeer = new Map<bigint, OrderedIndex<StyleSegment<Meta>>>();
  readonly #emptyMetas: ReadonlyMap<string, Meta> = new Map();

  get isEmpty(): boolean {
    return this.#segmentsByPeer.size === 0;
  }

  add(runs: readonly SequenceIdRun[], key: string, meta: Meta): void {
    for (const run of normalizeRuns(runs)) {
      const end = run.start.counter + run.length;
      if (run.length <= 0) continue;
      const segments = this.#segments(run.start.peer);
      this.#ensureBoundary(segments, run.start.counter);
      this.#ensureBoundary(segments, end);

      const existing: StyleSegment<Meta>[] = [];
      segments.forEachFrom(
        {
          start: run.start.counter,
          end: run.start.counter,
          histories: new Map(),
          latestMetas: undefined,
        },
        (segment) => {
          if (segment.start >= end) return false;
          existing.push(segment);
        },
      );

      let cursor = run.start.counter;
      for (const segment of existing) {
        if (cursor < segment.start) {
          const gap = {
            start: cursor,
            end: segment.start,
            histories: new Map<string, Meta[]>(),
            latestMetas: undefined,
          };
          this.#insertMeta(gap, key, meta);
          segments.add(gap);
        }
        this.#insertMeta(segment, key, meta);
        cursor = segment.end;
      }
      if (cursor < end) {
        const gap = {
          start: cursor,
          end,
          histories: new Map<string, Meta[]>(),
          latestMetas: undefined,
        };
        this.#insertMeta(gap, key, meta);
        segments.add(gap);
      }
    }
  }

  historyAt(id: SequenceId, key: string): readonly Meta[] | undefined {
    return this.#segmentAt(id)?.histories.get(key);
  }

  metasAt(
    id: SequenceId,
    version?: ReadonlyMap<bigint, number>,
  ): ReadonlyMap<string, Meta> {
    const segment = this.#segmentAt(id);
    if (segment === undefined) return this.#emptyMetas;
    if (version === undefined) return this.#latestMetas(segment);
    const metas = new Map<string, Meta>();
    for (const [key, history] of segment.histories) {
      const meta = latestIncluded(history, version);
      if (meta !== undefined) metas.set(key, meta);
    }
    return metas;
  }

  resolver(
    version?: ReadonlyMap<bigint, number>,
  ): (id: SequenceId) => ReadonlyMap<string, Meta> {
    const lastByPeer = new Map<bigint, SegmentLookup<Meta>>();
    const resolved = new Map<StyleSegment<Meta>, ReadonlyMap<string, Meta>>();
    return (id) => {
      let lookup = lastByPeer.get(id.peer);
      if (lookup === undefined || id.counter < lookup.start || id.counter >= lookup.end) {
        lookup = this.#lookupAt(id);
        lastByPeer.set(id.peer, lookup);
      }
      const segment = lookup.segment;
      if (segment === undefined) return this.#emptyMetas;
      if (version === undefined) return this.#latestMetas(segment);
      const cached = resolved.get(segment);
      if (cached !== undefined) return cached;
      const metas = new Map<string, Meta>();
      for (const [key, history] of segment.histories) {
        const meta = latestIncluded(history, version);
        if (meta !== undefined) metas.set(key, meta);
      }
      resolved.set(segment, metas);
      return metas;
    };
  }

  rangeHasKey(
    runs: readonly SequenceIdRun[],
    key: string,
    version?: ReadonlyMap<bigint, number>,
  ): boolean {
    for (const run of normalizeRuns(runs)) {
      const segments = this.#segmentsByPeer.get(run.start.peer);
      if (segments === undefined) continue;
      const end = run.start.counter + run.length;
      const first = Math.max(
        0,
        segments._lowerBoundBy((segment) => segment.start - run.start.counter) - 1,
      );
      for (let index = first; index < segments.size; index += 1) {
        const segment = segments.at(index)!;
        if (segment.start >= end) break;
        if (segment.end <= run.start.counter) continue;
        const meta = latestIncluded(segment.histories.get(key), version);
        if (meta !== undefined && meta.value !== null) return true;
      }
    }
    return false;
  }

  transitions(
    runs: readonly SequenceIdRun[],
    key: string,
    beforeVersion: ReadonlyMap<bigint, number> | undefined,
    afterVersion: ReadonlyMap<bigint, number> | undefined,
  ): TextStyleTransition<Meta>[] {
    const transitions: TextStyleTransition<Meta>[] = [];
    const append = (
      start: SequenceId,
      length: number,
      before: Meta | undefined,
      after: Meta | undefined,
    ): void => {
      if (before === after || length === 0) return;
      const previous = transitions.at(-1);
      if (
        previous !== undefined &&
        previous.before === before &&
        previous.after === after &&
        previous.run.start.peer === start.peer &&
        previous.run.start.counter + previous.run.length === start.counter
      ) {
        transitions[transitions.length - 1] = {
          run: { start: previous.run.start, length: previous.run.length + length },
          before,
          after,
        };
      } else {
        transitions.push({ run: { start: { ...start }, length }, before, after });
      }
    };

    for (const run of normalizeRuns(runs)) {
      const segments = this.#segmentsByPeer.get(run.start.peer);
      if (segments === undefined) continue;
      const end = run.start.counter + run.length;
      const first = Math.max(
        0,
        segments._lowerBoundBy((segment) => segment.start - run.start.counter) - 1,
      );
      for (let index = first; index < segments.size; index += 1) {
        const segment = segments.at(index)!;
        if (segment.start >= end) break;
        const start = Math.max(run.start.counter, segment.start);
        const segmentEnd = Math.min(end, segment.end);
        if (start >= segmentEnd) continue;
        append(
          { peer: run.start.peer, counter: start },
          segmentEnd - start,
          latestIncluded(segment.histories.get(key), beforeVersion),
          latestIncluded(segment.histories.get(key), afterVersion),
        );
      }
    }
    return transitions;
  }

  runsContainMeta(runs: readonly SequenceIdRun[], key: string, id: SequenceId): boolean {
    for (const run of runs) {
      const segments = this.#segmentsByPeer.get(run.start.peer);
      if (segments === undefined) return false;
      const end = run.start.counter + run.length;
      let cursor = run.start.counter;
      const first = Math.max(
        0,
        segments._lowerBoundBy((segment) => segment.start - cursor) - 1,
      );
      for (let index = first; index < segments.size && cursor < end; index += 1) {
        const segment = segments.at(index)!;
        if (segment.start > cursor || segment.start >= end) return false;
        if (segment.end <= cursor) continue;
        if (
          !segment.histories
            .get(key)
            ?.some(
              (item) =>
                item.startId.peer === id.peer && item.startId.counter === id.counter,
            )
        ) {
          return false;
        }
        cursor = Math.min(end, segment.end);
      }
      if (cursor < end) return false;
    }
    return true;
  }

  reset(): void {
    this.#segmentsByPeer.clear();
  }

  #segments(peer: bigint): OrderedIndex<StyleSegment<Meta>> {
    let segments = this.#segmentsByPeer.get(peer);
    if (segments === undefined) {
      segments = new OrderedIndex((left, right) => left.start - right.start);
      this.#segmentsByPeer.set(peer, segments);
    }
    return segments;
  }

  #segmentAt(id: SequenceId): StyleSegment<Meta> | undefined {
    return this.#lookupAt(id).segment;
  }

  #lookupAt(id: SequenceId): SegmentLookup<Meta> {
    const segments = this.#segmentsByPeer.get(id.peer);
    if (segments === undefined) {
      return {
        start: Number.MIN_SAFE_INTEGER,
        end: Number.MAX_SAFE_INTEGER,
        segment: undefined,
      };
    }
    const nextIndex = segments._lowerBoundBy(
      (segment) => segment.start - (id.counter + 1),
    );
    const previous = segments.at(nextIndex - 1);
    if (previous !== undefined && id.counter < previous.end) {
      return { start: previous.start, end: previous.end, segment: previous };
    }
    return {
      start: previous?.end ?? Number.MIN_SAFE_INTEGER,
      end: segments.at(nextIndex)?.start ?? Number.MAX_SAFE_INTEGER,
      segment: undefined,
    };
  }

  #ensureBoundary(segments: OrderedIndex<StyleSegment<Meta>>, counter: number): void {
    const index = segments._lowerBoundBy((segment) => segment.start - counter);
    if (segments.at(index)?.start === counter) return;
    const previous = segments.at(index - 1);
    if (previous === undefined || counter >= previous.end) return;
    segments.delete(previous);
    const right: StyleSegment<Meta> = {
      start: counter,
      end: previous.end,
      histories: cloneHistories(previous.histories),
      latestMetas: previous.latestMetas,
    };
    previous.end = counter;
    segments.add(previous);
    segments.add(right);
  }

  #insertMeta(segment: StyleSegment<Meta>, key: string, meta: Meta): void {
    let history = segment.histories.get(key);
    if (history === undefined) {
      history = [];
      segment.histories.set(key, history);
    }
    const index = lowerBoundMeta(history, meta);
    if (sameMeta(history[index], meta)) return;
    history.splice(index, 0, meta);
    segment.latestMetas = undefined;
  }

  #latestMetas(segment: StyleSegment<Meta>): ReadonlyMap<string, Meta> {
    if (segment.latestMetas !== undefined) return segment.latestMetas;
    const metas = new Map<string, Meta>();
    for (const [key, history] of segment.histories) {
      const meta = history.at(-1);
      if (meta !== undefined) metas.set(key, meta);
    }
    segment.latestMetas = metas;
    return metas;
  }
}

function cloneHistories<Meta>(
  histories: ReadonlyMap<string, readonly Meta[]>,
): Map<string, Meta[]> {
  return new Map([...histories].map(([key, history]) => [key, [...history]]));
}

function normalizeRuns(runs: readonly SequenceIdRun[]): SequenceIdRun[] {
  const sorted = runs
    .filter((run) => run.length > 0)
    .map((run) => ({ start: { ...run.start }, length: run.length }))
    .sort((left, right) =>
      left.start.peer < right.start.peer
        ? -1
        : left.start.peer > right.start.peer
          ? 1
          : left.start.counter - right.start.counter,
    );
  const merged: { start: SequenceId; length: number }[] = [];
  for (const run of sorted) {
    const previous = merged.at(-1);
    const end = run.start.counter + run.length;
    if (
      previous !== undefined &&
      previous.start.peer === run.start.peer &&
      run.start.counter <= previous.start.counter + previous.length
    ) {
      previous.length =
        Math.max(previous.start.counter + previous.length, end) - previous.start.counter;
    } else {
      merged.push(run);
    }
  }
  return merged;
}

function compareMeta(left: IndexedTextStyleMeta, right: IndexedTextStyleMeta): number {
  return (
    left.lamport - right.lamport ||
    (left.startId.peer < right.startId.peer
      ? -1
      : left.startId.peer > right.startId.peer
        ? 1
        : 0)
  );
}

function lowerBoundMeta<Meta extends IndexedTextStyleMeta>(
  history: readonly Meta[],
  meta: Meta,
): number {
  let low = 0;
  let high = history.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (compareMeta(history[middle]!, meta) < 0) low = middle + 1;
    else high = middle;
  }
  return low;
}

function latestIncluded<Meta extends IndexedTextStyleMeta>(
  history: readonly Meta[] | undefined,
  version: ReadonlyMap<bigint, number> | undefined,
): Meta | undefined {
  if (history === undefined) return undefined;
  if (version === undefined) return history.at(-1);
  for (let index = history.length - 1; index >= 0; index -= 1) {
    const meta = history[index]!;
    if (meta.startId.counter < (version.get(meta.startId.peer) ?? 0)) return meta;
  }
  return undefined;
}

function sameMeta<Meta extends IndexedTextStyleMeta>(
  left: Meta | undefined,
  right: Meta,
): boolean {
  return (
    left?.startId.peer === right.startId.peer &&
    left.startId.counter === right.startId.counter
  );
}
