import { isDeepStrictEqual } from "node:util";

import type {
  ContainerLike,
  DocLike,
  EngineModule,
  EventBatchLike,
  FrontiersLike,
  MapLike,
  MovableListLike,
  TextLike,
  UndoManagerLike,
  VersionVectorLike,
} from "./engine";

/**
 * Differential driver for loro.js against the Rust implementation.
 *
 * Every simulated peer is a pair of documents with the same peer ID, one per
 * engine. Local edits are applied to both halves; syncing exports bytes from
 * one peer and imports them into another, optionally crossing engines so each
 * engine also decodes the other's encoding. After each step both halves must
 * agree on the value, version, per-element metadata, and emitted events.
 */

export type EngineName = "rust" | "js";
export const ENGINES: readonly EngineName[] = ["rust", "js"];

export class Rng {
  #state: number;

  constructor(seed: number) {
    this.#state = seed >>> 0 || 1;
  }

  next(): number {
    // mulberry32
    this.#state = (this.#state + 0x6d2b79f5) >>> 0;
    let t = this.#state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296;
  }

  int(maxExclusive: number): number {
    return Math.floor(this.next() * maxExclusive);
  }

  bool(probability = 0.5): boolean {
    return this.next() < probability;
  }

  pick<T>(items: readonly T[]): T {
    return items[this.int(items.length)]!;
  }

  weighted<T extends string>(weights: Readonly<Partial<Record<T, number>>>): T {
    const entries = Object.entries(weights) as [T, number][];
    const total = entries.reduce((sum, [, weight]) => sum + weight, 0);
    let roll = this.next() * total;
    for (const [key, weight] of entries) {
      roll -= weight;
      if (roll < 0) return key;
    }
    return entries.at(-1)![0];
  }
}

export class Divergence extends Error {}

type Side<T> = Record<EngineName, T>;

export interface TwinOptions {
  readonly undo?: boolean;
}

export class Twin {
  readonly docs: Side<DocLike>;
  readonly events: Side<EventBatchLike[]> = { rust: [], js: [] };
  readonly undo: Side<UndoManagerLike> | undefined;
  /** Frontiers reached by this peer, shared by both halves once they agree. */
  readonly history: FrontiersLike[] = [];
  /** Set while `importBatch` runs; see `canonicalBatch`. */
  batchImport = false;
  /** List and Text values at the last comparison, to replay event deltas. */
  readonly lists = new Map<string, unknown[] | string>();

  constructor(
    readonly engines: Side<EngineModule>,
    readonly name: string,
    readonly peer: number,
    docs?: Side<DocLike>,
    options: TwinOptions = {},
  ) {
    this.docs = docs ?? {
      rust: new engines.rust.LoroDoc(),
      js: new engines.js.LoroDoc(),
    };
    for (const engine of ENGINES) {
      this.docs[engine].setPeerId(peer);
      this.docs[engine].subscribe((event) => {
        const batch = canonicalBatch(event, this.batchImport);
        if (batch.events.length > 0) this.events[engine].push(batch);
      });
    }
    this.undo =
      options.undo === false
        ? undefined
        : {
            rust: new engines.rust.UndoManager(this.docs.rust, { mergeInterval: 0 }),
            js: new engines.js.UndoManager(this.docs.js, { mergeInterval: 0 }),
          };
  }

  /** Runs `action` on both halves and requires them to both succeed or both throw. */
  both<T>(
    label: string,
    action: (doc: DocLike, engine: EngineName) => T,
  ): Side<T> | undefined {
    const results: Partial<Side<T>> = {};
    const errors: Partial<Side<unknown>> = {};
    for (const engine of ENGINES) {
      try {
        results[engine] = action(this.docs[engine], engine);
      } catch (error) {
        errors[engine] = error;
      }
    }
    if ((errors.rust === undefined) !== (errors.js === undefined)) {
      throw new Divergence(
        `${this.name}: ${label} ` +
          (errors.rust === undefined
            ? `succeeded in Rust but threw in loro.js: ${describeError(errors.js)}`
            : `threw in Rust but succeeded in loro.js: ${describeError(errors.rust)}`),
      );
    }
    return errors.rust === undefined ? (results as Side<T>) : undefined;
  }

  record(): void {
    this.history.push(this.docs.rust.frontiers());
  }
}

export function describeError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function vvToMap(vv: VersionVectorLike): Map<string, number> {
  return new Map([...vv.toJSON()].sort(([left], [right]) => compareStrings(left, right)));
}

export function sortedFrontiers(frontiers: FrontiersLike): string[] {
  return frontiers.map(({ peer, counter }) => `${counter}@${peer}`).sort(compareStrings);
}

function compareStrings(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

export function isContainer(value: unknown): value is ContainerLike {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as { kind?: unknown }).kind === "function" &&
    typeof (value as { id?: unknown }).id === "string"
  );
}

/** Replaces container handles with their IDs so values compare structurally. */
export function plain(value: unknown): unknown {
  if (isContainer(value)) return `container:${value.id}`;
  if (value instanceof Uint8Array) return { bytes: [...value] };
  if (Array.isArray(value)) return value.map(plain);
  if (value instanceof Map) {
    return Object.fromEntries(
      [...value].map(([key, item]) => [String(key), plain(item)]),
    );
  }
  if (typeof value === "object" && value !== null) {
    return Object.fromEntries(
      Object.entries(value)
        .filter(([, item]) => item !== undefined)
        .sort(([left], [right]) => compareStrings(left, right))
        .map(([key, item]) => [key, plain(item)]),
    );
  }
  return value;
}

type DeltaItem =
  | { readonly insert: unknown; readonly attributes?: unknown }
  | { readonly delete: number }
  | { readonly retain: number; readonly attributes?: unknown };

/**
 * Canonicalizes a list or text delta. Deletes and inserts at the same position
 * commute, so each gap between retains is rewritten as one delete followed by
 * the concatenated inserts.
 */
export function canonicalDelta(delta: readonly DeltaItem[], text: boolean): DeltaItem[] {
  const output: DeltaItem[] = [];
  let deleted = 0;
  let inserts: { value: unknown; attributes: unknown }[] = [];
  const flush = (): void => {
    if (deleted > 0) output.push({ delete: deleted });
    for (const insert of inserts) {
      const last = output.at(-1);
      if (
        last !== undefined &&
        "insert" in last &&
        isDeepStrictEqual(plain(last.attributes), plain(insert.attributes))
      ) {
        const merged = text
          ? (last.insert as string) + (insert.value as string)
          : [...(last.insert as unknown[]), ...(insert.value as unknown[])];
        output[output.length - 1] =
          insert.attributes === undefined
            ? { insert: merged }
            : { insert: merged, attributes: insert.attributes };
      } else {
        output.push(
          insert.attributes === undefined
            ? { insert: insert.value }
            : { insert: insert.value, attributes: insert.attributes },
        );
      }
    }
    deleted = 0;
    inserts = [];
  };
  for (const item of delta) {
    if ("insert" in item) {
      const attributes =
        item.attributes === undefined ||
        Object.keys(item.attributes as object).length === 0
          ? undefined
          : plain(item.attributes);
      inserts.push({ value: text ? item.insert : plain(item.insert), attributes });
    } else if ("delete" in item) {
      deleted += item.delete;
    } else {
      flush();
      const attributes =
        item.attributes === undefined ||
        Object.keys(item.attributes as object).length === 0
          ? undefined
          : plain(item.attributes);
      const last = output.at(-1);
      if (
        last !== undefined &&
        "retain" in last &&
        isDeepStrictEqual(last.attributes, attributes)
      ) {
        output[output.length - 1] =
          attributes === undefined
            ? { retain: last.retain + item.retain }
            : { retain: last.retain + item.retain, attributes };
      } else {
        output.push(
          attributes === undefined
            ? { retain: item.retain }
            : { retain: item.retain, attributes },
        );
      }
    }
  }
  flush();
  const last = output.at(-1);
  if (last !== undefined && "retain" in last && last.attributes === undefined)
    output.pop();
  return output;
}

export function canonicalDiff(diff: unknown): unknown {
  const typed = diff as { type: string; diff?: DeltaItem[]; updated?: unknown };
  if (typed.type === "list") {
    return { type: "list", diff: canonicalDelta(typed.diff ?? [], false) };
  }
  if (typed.type === "text") {
    return { type: "text", diff: canonicalDelta(typed.diff ?? [], true) };
  }
  return plain(diff);
}

/**
 * Normalizes an event batch for comparison. Two known differences unrelated to
 * the data model are folded away:
 *
 * - Rust labels checkout events with origin `"checkout"`; loro.js leaves the
 *   origin empty.
 * - Rust's multi-blob `importBatch` imports while detached and reattaches with a
 *   checkout, so its events say `by: "checkout"`; loro.js says `"import"`.
 * - Rust commits both undo and redo with origin `"undo"`; loro.js uses
 *   `"redo"` for redo.
 */
export function canonicalBatch(
  batch: EventBatchLike,
  batchImport = false,
): EventBatchLike {
  const by = batchImport && batch.by === "checkout" ? "import" : batch.by;
  const origin = batch.by === "checkout" ? "" : (batch.origin ?? "");
  return {
    by,
    origin: origin === "redo" ? "undo" : origin,
    events: batch.events
      .map((event) => ({
        target: event.target,
        path: [...event.path],
        diff: canonicalDiff(event.diff),
      }))
      .filter((event) => !isEmptyDiff(event.diff))
      .sort((left, right) => compareStrings(left.target, right.target)),
  };
}

function isEmptyDiff(diff: unknown): boolean {
  const typed = diff as { type: string; diff?: unknown[]; updated?: object };
  if (typed.type === "list" || typed.type === "text") return typed.diff?.length === 0;
  if (typed.type === "map") return Object.keys(typed.updated ?? {}).length === 0;
  return false;
}

export interface ContainerInfo {
  readonly id: string;
  readonly kind: string;
  readonly path: readonly (string | number)[];
}

/** Lists the reachable containers of `doc`, parents before children. */
export function reachableContainers(doc: DocLike): ContainerInfo[] {
  const output: ContainerInfo[] = [];
  const queue: ContainerInfo[] = Object.entries(doc.getShallowValue())
    .sort(([left], [right]) => compareStrings(left, right))
    .map(([name, id]) => ({ id, kind: kindOf(id), path: [name] }));
  while (queue.length > 0) {
    const info = queue.shift()!;
    output.push(info);
    const container = doc.getContainerById(info.id);
    if (container === undefined) continue;
    const shallow = container.getShallowValue();
    const children: [string | number, unknown][] = Array.isArray(shallow)
      ? shallow.map((value, index) => [index, value])
      : typeof shallow === "object" && shallow !== null
        ? Object.entries(shallow).sort(([left], [right]) => compareStrings(left, right))
        : [];
    for (const [key, value] of children) {
      if (typeof value === "string" && value.startsWith("cid:")) {
        queue.push({ id: value, kind: kindOf(value), path: [...info.path, key] });
      }
    }
  }
  return output;
}

function kindOf(id: string): string {
  return id.slice(id.lastIndexOf(":") + 1);
}

export interface MovableListMetadata {
  readonly creator: (string | undefined)[];
  readonly editor: (string | undefined)[];
  readonly mover: (string | undefined)[];
}

export function movableListMetadata(list: MovableListLike): MovableListMetadata {
  const creator: (string | undefined)[] = [];
  const editor: (string | undefined)[] = [];
  const mover: (string | undefined)[] = [];
  for (let index = 0; index < list.length; index += 1) {
    creator.push(list.getCreatorAt(index));
    editor.push(list.getLastEditorAt(index));
    mover.push(list.getLastMoverAt(index));
  }
  return { creator, editor, mover };
}

export interface CompareOptions {
  /** Compare getCreatorAt/getLastEditorAt/getLastMoverAt. */
  readonly metadata?: boolean;
  /** Compare the event batches collected since the last comparison. */
  readonly events?: boolean;
}

export function assertTwinAgrees(
  twin: Twin,
  context: string,
  options: CompareOptions = {},
): void {
  const fail = (message: string, rust: unknown, js: unknown): never => {
    throw new Divergence(
      `${twin.name} ${context}: ${message}\n  rust: ${stringify(rust)}\n  js:   ${stringify(js)}`,
    );
  };
  const { rust, js } = twin.docs;
  const rustValue = plain(rust.toJSON());
  const jsValue = plain(js.toJSON());
  if (!isDeepStrictEqual(rustValue, jsValue)) fail("values differ", rustValue, jsValue);
  if (rust.getPendingTxnLength() !== js.getPendingTxnLength()) {
    fail(
      "pending transaction lengths differ",
      rust.getPendingTxnLength(),
      js.getPendingTxnLength(),
    );
  }
  // Rust's oplog version already counts the pending transaction; loro.js's does
  // not. Only committed versions are compared.
  if (rust.getPendingTxnLength() === 0) {
    const rustVersion = vvToMap(rust.oplogVersion());
    const jsVersion = vvToMap(js.oplogVersion());
    if (!isDeepStrictEqual(rustVersion, jsVersion)) {
      fail("oplog versions differ", rustVersion, jsVersion);
    }
    const rustFrontiers = sortedFrontiers(rust.frontiers());
    const jsFrontiers = sortedFrontiers(js.frontiers());
    if (!isDeepStrictEqual(rustFrontiers, jsFrontiers)) {
      fail("frontiers differ", rustFrontiers, jsFrontiers);
    }
  }
  if (rust.isDetached() !== js.isDetached()) {
    fail("detached state differs", rust.isDetached(), js.isDetached());
  }
  const rustContainers = reachableContainers(rust);
  const jsContainers = reachableContainers(js);
  if (!isDeepStrictEqual(rustContainers, jsContainers)) {
    fail("reachable containers differ", rustContainers, jsContainers);
  }
  if (options.metadata !== false) {
    for (const info of rustContainers) {
      if (info.kind !== "MovableList") continue;
      const rustMeta = movableListMetadata(
        rust.getContainerById(info.id) as MovableListLike,
      );
      const jsMeta = movableListMetadata(js.getContainerById(info.id) as MovableListLike);
      if (!isDeepStrictEqual(rustMeta, jsMeta)) {
        fail(`movable-list metadata of ${info.id} differs`, rustMeta, jsMeta);
      }
    }
  }
  if (options.events !== false) {
    // loro.js also reports changes to child containers whose parent element is
    // already deleted; Rust does not. Compare only events of reachable targets.
    const reachable = new Set(rustContainers.map(({ id }) => id));
    const rustEvents = onlyReachable(twin.events.rust.splice(0), reachable);
    const jsEvents = onlyReachable(twin.events.js.splice(0), reachable);
    if (!isDeepStrictEqual(rustEvents, jsEvents)) {
      const reason = equivalentEvents(twin.lists, rustEvents, jsEvents);
      if (reason !== undefined) fail(`events differ (${reason})`, rustEvents, jsEvents);
      eventShapeDifferences.count += 1;
    }
  } else {
    twin.events.rust.length = 0;
    twin.events.js.length = 0;
  }
  twin.lists.clear();
  for (const info of rustContainers) {
    if (info.kind !== "MovableList" && info.kind !== "List" && info.kind !== "Text")
      continue;
    const value = rust.getContainerById(info.id)?.getShallowValue();
    if (Array.isArray(value) || typeof value === "string") twin.lists.set(info.id, value);
  }
}

/**
 * Event batches whose list or text deltas differ in shape but not in effect.
 * Rust derives an import's list event from the net change; loro.js composes the
 * change of each op, which can show an element that was hidden and shown again
 * within one import as a delete plus an insert of the same value. When several
 * imported moves bring an element back to its index, Rust's net change cancels
 * and it reports nothing, so sequence events without effect are dropped. After
 * a history replay loro.js diffs text before and after instead of using the ops.
 */
export const eventShapeDifferences = { count: 0 };

/** Returns why the batches are not equivalent, or undefined when they are. */
function equivalentEvents(
  lists: ReadonlyMap<string, unknown[] | string>,
  rustBatches: readonly EventBatchLike[],
  jsBatches: readonly EventBatchLike[],
): string | undefined {
  const rust = withoutNoOpSequenceEvents(lists, rustBatches);
  const js = withoutNoOpSequenceEvents(lists, jsBatches);
  if (rust === undefined || js === undefined) return "a list delta overruns";
  if (rust.length !== js.length) return "batch counts differ";
  const rustLists = new Map<string, unknown[] | string>();
  const jsLists = new Map<string, unknown[] | string>();
  for (let index = 0; index < rust.length; index += 1) {
    const left = rust[index]!;
    const right = js[index]!;
    if (left.by !== right.by || left.origin !== right.origin)
      return "batch labels differ";
    if (left.events.length !== right.events.length) return "event counts differ";
    for (let eventIndex = 0; eventIndex < left.events.length; eventIndex += 1) {
      const rustEvent = left.events[eventIndex]!;
      const jsEvent = right.events[eventIndex]!;
      if (rustEvent.target !== jsEvent.target) return "event targets differ";
      if (!isDeepStrictEqual(rustEvent.path, jsEvent.path)) return "event paths differ";
      const rustDiff = rustEvent.diff as { type: string; diff?: DeltaItem[] };
      const jsDiff = jsEvent.diff as { type: string; diff?: DeltaItem[] };
      const sequence =
        rustDiff.type === jsDiff.type &&
        (rustDiff.type === "list" || rustDiff.type === "text");
      if (!sequence) {
        if (!isDeepStrictEqual(rustDiff, jsDiff)) return `${rustDiff.type} diffs differ`;
        continue;
      }
      // Text attributes are compared structurally; only plain inserts are replayed.
      if (rustDiff.type === "text" && hasAttributes(rustDiff.diff!, jsDiff.diff!)) {
        return "text diffs with attributes differ";
      }
      const start = lists.get(rustEvent.target) ?? (rustDiff.type === "text" ? "" : []);
      const rustValue = applySequenceDelta(
        rustLists.get(rustEvent.target) ?? start,
        rustDiff.diff!,
      );
      const jsValue = applySequenceDelta(
        jsLists.get(jsEvent.target) ?? start,
        jsDiff.diff!,
      );
      if (rustValue === undefined || jsValue === undefined)
        return "a list delta overruns";
      if (!isDeepStrictEqual(rustValue, jsValue))
        return "list deltas have different effects";
      rustLists.set(rustEvent.target, rustValue);
      jsLists.set(jsEvent.target, jsValue);
    }
  }
  return undefined;
}

/** Drops list and text events that leave their target unchanged, then empty batches. */
function withoutNoOpSequenceEvents(
  lists: ReadonlyMap<string, unknown[] | string>,
  batches: readonly EventBatchLike[],
): EventBatchLike[] | undefined {
  const current = new Map(lists);
  const output: EventBatchLike[] = [];
  for (const batch of batches) {
    const events: EventBatchLike["events"][number][] = [];
    for (const event of batch.events) {
      const diff = event.diff as { type: string; diff?: DeltaItem[] };
      if ((diff.type !== "list" && diff.type !== "text") || hasAttributes(diff.diff!)) {
        events.push(event);
        continue;
      }
      const before = current.get(event.target) ?? (diff.type === "text" ? "" : []);
      const after = applySequenceDelta(before, diff.diff!);
      if (after === undefined) return undefined;
      current.set(event.target, after);
      if (!isDeepStrictEqual(before, after)) events.push(event);
    }
    if (events.length > 0) output.push({ ...batch, events });
  }
  return output;
}

function hasAttributes(...deltas: (readonly DeltaItem[])[]): boolean {
  return deltas.some((delta) =>
    delta.some((item) => "attributes" in item && item.attributes !== undefined),
  );
}

function applySequenceDelta(
  value: readonly unknown[] | string,
  delta: readonly DeltaItem[],
): unknown[] | string | undefined {
  if (typeof value !== "string") return applyListDelta(value, delta);
  // Text event indices count UTF-16 code units.
  const units = applyListDelta(
    value.split(""),
    delta.map((item) =>
      "insert" in item ? { insert: (item.insert as string).split("") } : item,
    ),
  );
  return units === undefined ? undefined : units.join("");
}

function applyListDelta(
  value: readonly unknown[],
  delta: readonly DeltaItem[],
): unknown[] | undefined {
  const output: unknown[] = [];
  let index = 0;
  for (const item of delta) {
    if ("retain" in item) {
      if (index + item.retain > value.length) return undefined;
      output.push(...value.slice(index, index + item.retain));
      index += item.retain;
    } else if ("delete" in item) {
      if (index + item.delete > value.length) return undefined;
      index += item.delete;
    } else {
      // Shallow values name child containers by ID; events carry handles.
      output.push(
        ...(item.insert as unknown[]).map((inserted) =>
          typeof inserted === "string" && inserted.startsWith("container:")
            ? inserted.slice("container:".length)
            : inserted,
        ),
      );
    }
  }
  output.push(...value.slice(index));
  return output;
}

function onlyReachable(
  batches: readonly EventBatchLike[],
  reachable: ReadonlySet<string>,
): EventBatchLike[] {
  return batches
    .map((batch) => ({
      ...batch,
      events: batch.events.filter((event) => reachable.has(event.target)),
    }))
    .filter((batch) => batch.events.length > 0);
}

export function stringify(value: unknown): string {
  return JSON.stringify(value, (_key, item: unknown) =>
    item instanceof Map ? Object.fromEntries(item) : item,
  );
}

export function movableList(doc: DocLike, id: string): MovableListLike {
  return doc.getContainerById(id) as MovableListLike;
}

export function mapContainer(doc: DocLike, id: string): MapLike {
  return doc.getContainerById(id) as MapLike;
}

export function textContainer(doc: DocLike, id: string): TextLike {
  return doc.getContainerById(id) as TextLike;
}
