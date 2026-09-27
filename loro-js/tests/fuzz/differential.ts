// Differential fuzzing harness for the TypeScript runtime.
//
// A scenario is a list of abstract actions (local edits, update exchanges in
// several orders, checkouts). Each action carries random 32-bit parameters that
// are resolved against the acting document when it runs, so removing actions
// while minimizing keeps the remaining ones meaningful.
//
// Oracles:
// - every peer converges with a document that replays the same updates at once
//   and with one that imports each change separately in a shuffled order;
// - events mirrored into a container-keyed model reproduce `toJSON()` after
//   every local commit, import, and checkout;
// - `checkout(F)` equals a fresh document that imports only the changes in F;
// - a full snapshot and shallow snapshots equal the full-history document,
//   including checkouts into the retained range;
// - optionally, a Rust/WASM `loro-crdt` build replays the same updates, checks
//   out the same versions, and exchanges shallow snapshots with the runtime.
import {
  LoroCounter,
  LoroDoc,
  LoroList,
  LoroMap,
  LoroMovableList,
  LoroText,
  LoroTree,
} from "../../src/runtime";
import type { Container } from "../../src/runtime";
import type {
  ContainerID,
  ContainerType,
  Delta,
  Frontiers,
  LoroEventBatch,
  OpId,
  TreeDiffItem,
  TreeID,
} from "../../src/runtime/types";

export interface FuzzFeatures {
  readonly map: boolean;
  readonly list: boolean;
  readonly text: boolean;
  readonly tree: boolean;
  readonly counter: boolean;
  readonly movableList: boolean;
  /** Rich-text `mark`/`unmark`. */
  readonly textStyle: boolean;
  /** Child containers inside maps, lists, and tree metadata. */
  readonly nested: boolean;
  /** Import full snapshots into documents that already have history. */
  readonly snapshotSync: boolean;
}

export const ALL_FEATURES: FuzzFeatures = {
  map: true,
  list: true,
  text: true,
  tree: true,
  counter: true,
  movableList: true,
  textStyle: true,
  nested: true,
  snapshotSync: false,
};

export const CORE_FEATURES: FuzzFeatures = {
  ...ALL_FEATURES,
  movableList: false,
  textStyle: false,
};

export interface FuzzChecks {
  readonly events: boolean;
  readonly checkout: boolean;
  readonly snapshot: boolean;
  readonly shallow: boolean;
}

export const ALL_CHECKS: FuzzChecks = {
  events: true,
  checkout: true,
  snapshot: true,
  shallow: true,
};

export type SyncMode = "update" | "full" | "shuffled" | "batch" | "snapshot";

export type Action =
  | {
      readonly kind: "op";
      readonly peer: number;
      readonly target: number;
      readonly op: number;
      readonly a: number;
      readonly b: number;
      readonly commit: boolean;
    }
  | {
      readonly kind: "sync";
      readonly from: number;
      readonly to: number;
      readonly mode: SyncMode;
      readonly r: number;
    }
  | {
      readonly kind: "checkout";
      readonly peer: number;
      readonly r: number;
      readonly r2: number;
      readonly stay: boolean;
    };

export interface Scenario {
  readonly seed: number;
  readonly peers: number;
  readonly features: FuzzFeatures;
  readonly actions: readonly Action[];
}

/** A structural subset of the `loro-crdt` WASM `LoroDoc` used by the Rust oracle. */
export interface RustDoc {
  setPeerId(peer: bigint | number): void;
  import(bytes: Uint8Array): unknown;
  importBatch(blobs: Uint8Array[]): unknown;
  export(mode: unknown): Uint8Array;
  toJSON(): unknown;
  commit(): void;
  checkout(frontiers: Frontiers): void;
  checkoutToLatest(): void;
  attach(): void;
  oplogVersion(): unknown;
  shallowSinceFrontiers(): Frontiers;
  configTextStyle(styles: unknown): void;
  subscribe(listener: (batch: LoroEventBatch) => void): unknown;
  getContainerById(id: ContainerID): unknown;
}

export interface RustOracle {
  newDoc(): RustDoc;
  /** A detached container of the given type. */
  newContainer(type: ContainerType): unknown;
  /** Delivers queued WASM events (`callPendingEvents`). */
  flushEvents(): void;
}

export interface FuzzFailure {
  readonly check: string;
  readonly signature: string;
  readonly message: string;
  readonly trace: readonly string[];
}

export class FuzzMismatch extends Error {
  constructor(
    readonly check: string,
    readonly key: string,
    message: string,
  ) {
    super(message);
  }
}

// ---------------------------------------------------------------------------
// Random generation

export class Random {
  #state: number;

  constructor(seed: number) {
    this.#state = (seed ^ 0x9e37_79b9) >>> 0 || 1;
    for (let index = 0; index < 4; index += 1) this.next();
  }

  next(): number {
    let x = this.#state;
    x ^= x << 13;
    x >>>= 0;
    x ^= x >>> 17;
    x ^= x << 5;
    x >>>= 0;
    this.#state = x;
    return x;
  }

  below(limit: number): number {
    return limit <= 0 ? 0 : this.next() % limit;
  }

  chance(numerator: number, denominator: number): boolean {
    return this.below(denominator) < numerator;
  }
}

export interface GenerateOptions {
  readonly peers?: number;
  readonly actions?: number;
  readonly features?: FuzzFeatures;
}

export function generateScenario(seed: number, options: GenerateOptions = {}): Scenario {
  const random = new Random(seed);
  const peers = options.peers ?? 2 + random.below(3);
  const count = options.actions ?? 30 + random.below(50);
  const features = options.features ?? CORE_FEATURES;
  const syncModes: SyncMode[] = ["update", "update", "full", "shuffled", "batch"];
  if (features.snapshotSync) syncModes.push("snapshot");
  const actions: Action[] = [];
  for (let index = 0; index < count; index += 1) {
    const roll = random.below(100);
    if (roll < 66) {
      actions.push({
        kind: "op",
        peer: random.below(peers),
        target: random.next(),
        op: random.next(),
        a: random.next(),
        b: random.next(),
        commit: random.chance(3, 4),
      });
    } else if (roll < 91) {
      const from = random.below(peers);
      let to = random.below(peers - 1);
      if (to >= from) to += 1;
      actions.push({
        kind: "sync",
        from,
        to,
        mode: syncModes[random.below(syncModes.length)]!,
        r: random.next(),
      });
    } else {
      actions.push({
        kind: "checkout",
        peer: random.below(peers),
        r: random.next(),
        r2: random.next(),
        stay: random.chance(1, 3),
      });
    }
  }
  return { seed, peers, features, actions };
}

// ---------------------------------------------------------------------------
// Value normalization

const ROOTS: readonly {
  name: string;
  type: ContainerType;
  feature: keyof FuzzFeatures;
}[] = [
  { name: "m", type: "Map", feature: "map" },
  { name: "l", type: "List", feature: "list" },
  { name: "t", type: "Text", feature: "text" },
  { name: "tr", type: "Tree", feature: "tree" },
  { name: "c", type: "Counter", feature: "counter" },
  { name: "ml", type: "MovableList", feature: "movableList" },
];

const TEXT_STYLES = {
  bold: { expand: "after" },
  link: { expand: "none" },
  comment: { expand: "both" },
  em: { expand: "before" },
} as const;

function sortValue(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sortValue);
  if (value instanceof Uint8Array) return Array.from(value);
  if (value !== null && typeof value === "object") {
    const output: Record<string, unknown> = {};
    for (const key of Object.keys(value).sort()) {
      const child = (value as Record<string, unknown>)[key];
      if (child !== undefined) output[key] = sortValue(child);
    }
    return output;
  }
  return value;
}

function isEmptyValue(value: unknown): boolean {
  if (value === "" || value === 0 || value === null || value === undefined) return true;
  if (Array.isArray(value)) return value.length === 0;
  if (typeof value === "object") return Object.keys(value as object).length === 0;
  return false;
}

/** Deep JSON with sorted keys and empty root containers removed. */
export function normalizeJson(value: unknown): Record<string, unknown> {
  const sorted = sortValue(value) as Record<string, unknown>;
  const output: Record<string, unknown> = {};
  for (const [key, child] of Object.entries(sorted)) {
    if (!isEmptyValue(child)) output[key] = child;
  }
  return output;
}

interface ContainerLike {
  readonly id: ContainerID;
  kind(): string;
}

function isContainerLike(value: unknown): value is ContainerLike {
  return (
    value !== null &&
    typeof value === "object" &&
    typeof (value as { kind?: unknown }).kind === "function" &&
    typeof (value as { id?: unknown }).id === "string"
  );
}

/** Rich-text deltas of every text reachable through maps and lists, keyed by id. */
function collectTextDeltas(
  doc: {
    getContainerById(id: ContainerID): unknown;
  },
  json: unknown,
): Record<string, unknown> {
  const output: Record<string, unknown> = {};
  const visit = (handle: unknown): void => {
    if (!isContainerLike(handle)) return;
    const kind = handle.kind();
    if (kind === "Text") {
      output[handle.id] = sortValue((handle as unknown as LoroText).toDelta());
    } else if (kind === "Map") {
      const map = handle as unknown as LoroMap;
      for (const key of map.keys().sort()) visit(map.get(key));
    } else if (kind === "List" || kind === "MovableList") {
      const list = handle as unknown as LoroList;
      for (let index = 0; index < list.length; index += 1) visit(list.get(index));
    }
  };
  for (const root of ROOTS) {
    if (json !== null && typeof json === "object" && root.name in json) {
      visit(doc.getContainerById(`cid:root-${root.name}:${root.type}`));
    }
  }
  return output;
}

export interface NormalizedValue {
  readonly json: Record<string, unknown>;
  readonly texts?: Record<string, unknown>;
}

export function docValue(
  doc: { toJSON(): unknown; getContainerById(id: ContainerID): unknown },
  withStyles: boolean,
): NormalizedValue {
  const raw = doc.toJSON();
  const json = normalizeJson(raw);
  if (!withStyles) return { json };
  const texts = collectTextDeltas(doc, raw);
  for (const key of Object.keys(texts)) {
    const delta = texts[key] as { attributes?: unknown }[];
    if (!delta.some((item) => item.attributes !== undefined)) delete texts[key];
  }
  return { json, texts };
}

function firstDifferenceKey(left: NormalizedValue, right: NormalizedValue): string {
  const keys = new Set([...Object.keys(left.json), ...Object.keys(right.json)]);
  for (const key of [...keys].sort()) {
    if (JSON.stringify(left.json[key]) !== JSON.stringify(right.json[key])) return key;
  }
  if (JSON.stringify(left.texts) !== JSON.stringify(right.texts)) return "style";
  return "?";
}

function describeDifference(left: unknown, right: unknown): string {
  const a = String(JSON.stringify(left));
  const b = String(JSON.stringify(right));
  if (a.length + b.length < 1600) return `\n  left:  ${a}\n  right: ${b}`;
  let start = 0;
  while (start < a.length && a[start] === b[start]) start += 1;
  const from = Math.max(0, start - 160);
  return `\n  left:  …${a.slice(from, start + 400)}\n  right: …${b.slice(from, start + 400)}`;
}

export function assertSameValue(
  check: string,
  left: NormalizedValue,
  right: NormalizedValue,
  context = "",
): void {
  if (JSON.stringify(left) === JSON.stringify(right)) return;
  const key = firstDifferenceKey(left, right);
  const leftPart = key === "style" ? left.texts : left.json[key];
  const rightPart = key === "style" ? right.texts : right.json[key];
  throw new FuzzMismatch(
    check,
    key,
    `${check} at root "${key}"${context}${describeDifference(leftPart, rightPart)}`,
  );
}

// ---------------------------------------------------------------------------
// Event mirror

type MirrorValue = unknown;

interface MirrorRef {
  readonly __container: ContainerID;
}

interface TreeMirror {
  readonly kind: "Tree";
  readonly parent: Map<TreeID, TreeID | undefined>;
  readonly position: Map<TreeID, string>;
  readonly children: Map<TreeID | "", TreeID[]>;
}

type MirrorState =
  | { readonly kind: "Map"; readonly value: Map<string, MirrorValue> }
  | { readonly kind: "List" | "MovableList"; value: MirrorValue[] }
  | { readonly kind: "Text"; value: string }
  | { readonly kind: "Counter"; value: number }
  | TreeMirror;

function containerTypeOf(id: ContainerID): ContainerType {
  return id.slice(id.lastIndexOf(":") + 1) as ContainerType;
}

function emptyMirror(id: ContainerID): MirrorState {
  const kind = containerTypeOf(id);
  switch (kind) {
    case "Map":
      return { kind, value: new Map() };
    case "List":
    case "MovableList":
      return { kind, value: [] };
    case "Text":
      return { kind, value: "" };
    case "Counter":
      return { kind, value: 0 };
    case "Tree":
      return { kind, parent: new Map(), position: new Map(), children: new Map() };
  }
}

function treeMetaId(node: TreeID): ContainerID {
  return `cid:${node}:Map` as ContainerID;
}

class MirrorError extends Error {}

/**
 * Applies event batches to a model keyed by container ID, with the same
 * container-reset rules as the Rust fuzzer's `ContainerTracker`: attaching a
 * container value starts that child from an empty state, while a movable-list
 * move keeps the moved child's state.
 */
export class EventMirror {
  readonly #states = new Map<ContainerID, MirrorState>();

  apply(batch: LoroEventBatch): void {
    for (const event of batch.events) {
      const state = this.#state(event.target);
      const diff = event.diff;
      switch (diff.type) {
        case "map":
          this.#applyMap(state, diff.updated);
          break;
        case "list":
          this.#applyList(state, diff.diff as Delta<unknown[]>[]);
          break;
        case "text":
          this.#applyText(state, diff.diff);
          break;
        case "counter":
          if (state.kind !== "Counter")
            throw new MirrorError(`counter diff on ${state.kind}`);
          state.value += diff.increment;
          break;
        case "tree":
          this.#applyTree(state, diff.diff);
          break;
      }
    }
  }

  toJSON(
    roots: readonly { name: string; type: ContainerType }[],
  ): Record<string, unknown> {
    const output: Record<string, unknown> = {};
    for (const root of roots) {
      const id = `cid:root-${root.name}:${root.type}` as ContainerID;
      if (this.#states.has(id)) output[root.name] = this.#json(id);
    }
    return output;
  }

  #state(id: ContainerID): MirrorState {
    let state = this.#states.get(id);
    if (state === undefined) {
      state = emptyMirror(id);
      this.#states.set(id, state);
    }
    return state;
  }

  #attach(value: unknown, moved: Set<ContainerID>): MirrorValue {
    if (!isContainerLike(value)) return value;
    const id = value.id;
    if (!moved.has(id)) this.#states.set(id, emptyMirror(id));
    return { __container: id } satisfies MirrorRef;
  }

  #applyMap(state: MirrorState, updated: Readonly<Record<string, unknown>>): void {
    if (state.kind !== "Map") throw new MirrorError(`map diff on ${state.kind}`);
    for (const [key, value] of Object.entries(updated)) {
      if (value === undefined) state.value.delete(key);
      else state.value.set(key, this.#attach(value, new Set()));
    }
  }

  #applyList(state: MirrorState, delta: readonly Delta<unknown[]>[]): void {
    if (state.kind !== "List" && state.kind !== "MovableList") {
      throw new MirrorError(`list diff on ${state.kind}`);
    }
    const removed = new Set<ContainerID>();
    if (state.kind === "MovableList") {
      let index = 0;
      for (const item of delta) {
        if ("retain" in item) index += item.retain;
        else if ("delete" in item) {
          for (const value of state.value.slice(index, index + item.delete)) {
            if (value !== null && typeof value === "object" && "__container" in value) {
              removed.add((value as MirrorRef).__container);
            }
          }
          index += item.delete;
        }
      }
    }
    const next: MirrorValue[] = [];
    let index = 0;
    for (const item of delta) {
      if ("retain" in item) {
        if (index + item.retain > state.value.length) {
          throw new MirrorError(
            `list retain ${item.retain} beyond ${state.value.length}`,
          );
        }
        next.push(...state.value.slice(index, index + item.retain));
        index += item.retain;
      } else if ("delete" in item) {
        if (index + item.delete > state.value.length) {
          throw new MirrorError(
            `list delete ${item.delete} beyond ${state.value.length}`,
          );
        }
        index += item.delete;
      } else {
        for (const value of item.insert) next.push(this.#attach(value, removed));
      }
    }
    next.push(...state.value.slice(index));
    state.value = next;
  }

  #applyText(state: MirrorState, delta: readonly Delta<string>[]): void {
    if (state.kind !== "Text") throw new MirrorError(`text diff on ${state.kind}`);
    let output = "";
    const chars = Array.from(state.value);
    // Event text positions are UTF-16 code units in the JS API.
    let utf16 = 0;
    let charIndex = 0;
    const takeUtf16 = (count: number): string => {
      let taken = "";
      const target = utf16 + count;
      while (utf16 < target) {
        const char = chars[charIndex];
        if (char === undefined)
          throw new MirrorError(`text span beyond ${state.value.length}`);
        taken += char;
        utf16 += char.length;
        charIndex += 1;
      }
      if (utf16 !== target) throw new MirrorError("text span splits a surrogate pair");
      return taken;
    };
    for (const item of delta) {
      if ("retain" in item) {
        output += takeUtf16(item.retain);
      } else if ("delete" in item) {
        takeUtf16(item.delete);
      } else {
        output += item.insert;
      }
    }
    output += chars.slice(charIndex).join("");
    state.value = output;
  }

  #applyTree(state: MirrorState, diff: readonly TreeDiffItem[]): void {
    if (state.kind !== "Tree") throw new MirrorError(`tree diff on ${state.kind}`);
    const childrenOf = (parent: TreeID | undefined): TreeID[] => {
      const key = parent ?? "";
      let list = state.children.get(key);
      if (list === undefined) {
        list = [];
        state.children.set(key, list);
      }
      return list;
    };
    const insert = (target: TreeID, parent: TreeID | undefined, index: number): void => {
      if (parent !== undefined && !state.parent.has(parent)) {
        throw new MirrorError(`tree parent ${parent} of ${target} does not exist`);
      }
      const siblings = childrenOf(parent);
      if (index > siblings.length) {
        throw new MirrorError(
          `tree index ${index} beyond ${siblings.length} for ${target}`,
        );
      }
      siblings.splice(index, 0, target);
      state.parent.set(target, parent);
    };
    const detach = (target: TreeID): void => {
      if (!state.parent.has(target))
        throw new MirrorError(`tree node ${target} is absent`);
      const siblings = childrenOf(state.parent.get(target));
      const index = siblings.indexOf(target);
      if (index < 0) throw new MirrorError(`tree node ${target} is not under its parent`);
      siblings.splice(index, 1);
    };
    for (const item of diff) {
      if (item.action === "create") {
        if (state.parent.has(item.target)) {
          throw new MirrorError(`tree node ${item.target} already exists`);
        }
        insert(item.target, item.parent, item.index);
        state.position.set(item.target, item.fractionalIndex);
        this.#states.set(treeMetaId(item.target), emptyMirror(treeMetaId(item.target)));
      } else if (item.action === "move") {
        detach(item.target);
        insert(item.target, item.parent, item.index);
        state.position.set(item.target, item.fractionalIndex);
      } else {
        detach(item.target);
        const stack = [item.target];
        while (stack.length > 0) {
          const node = stack.pop()!;
          stack.push(...(state.children.get(node) ?? []));
          state.children.delete(node);
          state.parent.delete(node);
          state.position.delete(node);
        }
      }
    }
  }

  #json(id: ContainerID): unknown {
    const state = this.#states.get(id) ?? emptyMirror(id);
    const value = (item: MirrorValue): unknown =>
      item !== null && typeof item === "object" && "__container" in item
        ? this.#json((item as MirrorRef).__container)
        : item;
    switch (state.kind) {
      case "Map": {
        const output: Record<string, unknown> = {};
        for (const [key, item] of state.value) output[key] = value(item);
        return output;
      }
      case "List":
      case "MovableList":
        return state.value.map(value);
      case "Text":
      case "Counter":
        return state.value;
      case "Tree": {
        const nodes = (parent: TreeID | undefined): unknown[] =>
          (state.children.get(parent ?? "") ?? []).map((node, index) => ({
            id: node,
            parent: parent ?? null,
            index,
            fractional_index: state.position.get(node),
            meta: this.#json(treeMetaId(node)),
            children: nodes(node),
          }));
        return nodes(undefined);
      }
    }
  }
}

// ---------------------------------------------------------------------------
// Scenario execution

interface Mirrored<D> {
  readonly doc: D;
  readonly mirror: EventMirror;
  mirrorError: unknown;
}

interface Peer extends Mirrored<LoroDoc> {
  readonly name: string;
  readonly rust: Mirrored<RustDoc> | undefined;
}

/** A local edit resolved against the acting document. */
interface ConcreteOp {
  readonly target: ContainerID;
  readonly method: string;
  readonly args: readonly unknown[];
}

interface NewContainer {
  readonly __new: ContainerType;
}

function isNewContainer(value: unknown): value is NewContainer {
  return value !== null && typeof value === "object" && "__new" in value;
}

function enabledRoots(features: FuzzFeatures): typeof ROOTS {
  return ROOTS.filter((root) => features[root.feature]);
}

function scalar(seed: number): unknown {
  switch (seed % 5) {
    case 0:
      return seed % 97;
    case 1:
      return `s${seed % 31}`;
    case 2:
      return (seed & 8) !== 0;
    case 3:
      return null;
    default:
      return { n: seed % 7 };
  }
}

const TEXT_PIECES = ["a", "bc", "XYZ", "中", "😀", "e😀f", "\n", "hello"];

function textPiece(seed: number): string {
  return TEXT_PIECES[seed % TEXT_PIECES.length]!;
}

function childContainerTypes(features: FuzzFeatures): ContainerType[] {
  const types: ContainerType[] = [];
  if (features.map) types.push("Map", "Map");
  if (features.list) types.push("List");
  if (features.text) types.push("Text");
  if (features.counter) types.push("Counter");
  if (features.tree) types.push("Tree");
  if (features.movableList) types.push("MovableList");
  return types;
}

function newDetached(type: ContainerType): Container {
  switch (type) {
    case "Map":
      return new LoroMap();
    case "List":
      return new LoroList();
    case "Text":
      return new LoroText();
    case "Counter":
      return new LoroCounter();
    case "Tree":
      return new LoroTree();
    case "MovableList":
      return new LoroMovableList();
  }
}

function frontiersLiteral(frontiers: Frontiers): string {
  return JSON.stringify(frontiers.map((id) => ({ peer: id.peer, counter: id.counter })));
}

const ROOT_GETTERS: Record<ContainerType, string> = {
  Map: "getMap",
  List: "getList",
  Text: "getText",
  Tree: "getTree",
  Counter: "getCounter",
  MovableList: "getMovableList",
};

function containerExpression(doc: string, id: ContainerID): string {
  const root = /^cid:root-(.*):(\w+)$/.exec(id);
  if (root !== null) {
    return `${doc}.${ROOT_GETTERS[root[2] as ContainerType]}(${JSON.stringify(root[1])})`;
  }
  return `(${doc}.getContainerById(${JSON.stringify(id)}) as any)`;
}

function renderArgument(value: unknown): string {
  if (isNewContainer(value)) return `new Loro${value.__new}()`;
  if (value === undefined) return "undefined";
  return JSON.stringify(value);
}

function renderOp(doc: string, op: ConcreteOp): string {
  return `${containerExpression(doc, op.target)}.${op.method}(${op.args.map(renderArgument).join(", ")});`;
}

function executeOp(
  doc: { getContainerById(id: ContainerID): unknown },
  op: ConcreteOp,
  create: (type: ContainerType) => unknown,
): string | undefined {
  try {
    const container = doc.getContainerById(op.target) as Record<
      string,
      (...args: unknown[]) => unknown
    >;
    const args = op.args.map((arg) => (isNewContainer(arg) ? create(arg.__new) : arg));
    container[op.method]!(...args);
    return undefined;
  } catch (error) {
    return String((error as Error)?.message ?? error);
  }
}

function sortedChanges(doc: LoroDoc): [string, { counter: number; length: number }[]][] {
  return [...doc.getAllChanges().entries()].sort(([a], [b]) => (a < b ? -1 : 1));
}

export interface RunOptions {
  readonly checks?: FuzzChecks;
  readonly rust?: RustOracle | undefined;
  /** Run every action on Rust peers too and compare after each action. */
  readonly lockstep?: boolean;
  /** Upper bound on versions checked out after the scenario. */
  readonly maxCheckoutVersions?: number;
  readonly onTrace?: ((line: string) => void) | undefined;
  /** Check only the Rust event mirrors and values; validates the harness itself. */
  readonly validateHarness?: boolean;
}

class Runner {
  readonly peers: Peer[] = [];
  readonly trace: string[] = [];
  readonly roots: typeof ROOTS;
  readonly checks: FuzzChecks;
  readonly rust: RustOracle | undefined;
  readonly lockstep: boolean;
  rustReplay: Mirrored<RustDoc> | undefined;

  constructor(
    readonly scenario: Scenario,
    readonly options: RunOptions,
  ) {
    this.roots = enabledRoots(scenario.features);
    this.checks = options.checks ?? ALL_CHECKS;
    this.rust = options.rust;
    this.lockstep = options.lockstep === true && options.rust !== undefined;
    for (let index = 0; index < scenario.peers; index += 1) {
      const doc = this.newDoc(index + 1);
      let rust: Mirrored<RustDoc> | undefined;
      if (this.lockstep) {
        const rustDoc = this.newRustDoc()!;
        rustDoc.setPeerId(BigInt(index + 1));
        rust = { doc: rustDoc, mirror: new EventMirror(), mirrorError: undefined };
        if (this.checks.events) this.subscribe(rust);
      }
      const peer: Peer = {
        name: `p${index + 1}`,
        doc,
        mirror: new EventMirror(),
        mirrorError: undefined,
        rust,
      };
      if (this.checks.events) this.subscribe(peer);
      this.peers.push(peer);
    }
  }

  get withStyles(): boolean {
    return this.scenario.features.textStyle;
  }

  log(line: string): void {
    this.trace.push(line);
    this.options.onTrace?.(line);
  }

  newDoc(peer?: number): LoroDoc {
    const doc = new LoroDoc();
    if (peer !== undefined) doc.setPeerId(peer);
    if (this.scenario.features.textStyle) doc.configTextStyle(TEXT_STYLES);
    return doc;
  }

  newRustDoc(): RustDoc | undefined {
    const doc = this.rust?.newDoc();
    if (doc !== undefined && this.scenario.features.textStyle) {
      doc.configTextStyle(TEXT_STYLES);
    }
    return doc;
  }

  subscribe(
    target: Mirrored<{ subscribe(listener: (batch: LoroEventBatch) => void): unknown }>,
  ): void {
    target.doc.subscribe((batch) => {
      if (target.mirrorError !== undefined) return;
      try {
        target.mirror.apply(batch);
      } catch (error) {
        target.mirrorError = error;
      }
    });
  }

  value(doc: LoroDoc): NormalizedValue {
    return docValue(doc, this.withStyles);
  }

  rustValue(doc: RustDoc): NormalizedValue {
    return docValue(doc, this.withStyles);
  }

  checkMirror(
    target: Mirrored<{ toJSON(): unknown }>,
    label: string,
    check = "event",
  ): void {
    if (!this.checks.events) return;
    if (this.options.validateHarness === true && !check.startsWith("rust")) return;
    if (target.mirrorError !== undefined) {
      const message = String((target.mirrorError as Error).message ?? target.mirrorError);
      throw new FuzzMismatch(
        `${check}-invalid`,
        message.replace(/\d+(@\d+)?/g, "N"),
        `${label}: ${message}`,
      );
    }
    const mirrored = normalizeJson(sortValue(target.mirror.toJSON(this.roots)));
    const actual = normalizeJson(target.doc.toJSON());
    assertSameValue(
      `${check}-mirror`,
      { json: mirrored },
      { json: actual },
      ` (${label})`,
    );
  }

  /** Compares a peer with its Rust twin after an action in lockstep mode. */
  checkPeer(peer: Peer, label: string): void {
    this.checkMirror(peer, label);
    if (peer.rust === undefined) return;
    this.rust!.flushEvents();
    assertSameValue(
      "lockstep",
      this.value(peer.doc),
      this.rustValue(peer.rust.doc),
      ` (${peer.name} vs Rust after ${label})`,
    );
    this.checkMirror(peer.rust, `Rust ${label}`, "rust-event");
  }

  // -- local edits -----------------------------------------------------------

  containers(doc: LoroDoc): ContainerLike[] {
    const output: ContainerLike[] = [];
    const queue: ContainerLike[] = [];
    for (const root of this.roots) {
      queue.push(doc.getContainerById(`cid:root-${root.name}:${root.type}`) as never);
    }
    while (queue.length > 0 && output.length < 32) {
      const container = queue.shift()!;
      output.push(container);
      const kind = container.kind();
      if (kind === "Map") {
        const map = container as unknown as LoroMap;
        for (const key of map.keys().sort()) {
          const child = map.get(key);
          if (isContainerLike(child)) queue.push(child);
        }
      } else if (kind === "List" || kind === "MovableList") {
        const list = container as unknown as LoroList;
        for (let index = 0; index < list.length; index += 1) {
          const child = list.get(index);
          if (isContainerLike(child)) queue.push(child);
        }
      } else if (kind === "Tree" && this.scenario.features.nested) {
        const tree = container as unknown as LoroTree;
        const nodes = tree.getNodes().sort((a, b) => (a.id < b.id ? -1 : 1));
        for (const node of nodes) queue.push(node.data as unknown as ContainerLike);
      }
    }
    return output;
  }

  /** Resolves an abstract edit against `doc`; undefined when it is a no-op. */
  concreteOp(
    doc: LoroDoc,
    action: Extract<Action, { kind: "op" }>,
  ): ConcreteOp | undefined {
    const containers = this.containers(doc);
    const container = containers[action.target % containers.length]!;
    const target = container.id;
    const features = this.scenario.features;
    const types = childContainerTypes(features);
    const childType: NewContainer = { __new: types[action.b % types.length]! };
    const { a, b } = action;
    switch (container.kind()) {
      case "Map": {
        const key = `k${a % 5}`;
        const choice = action.op % 5;
        if (choice === 0) return { target, method: "delete", args: [key] };
        if (choice === 1 && features.nested) {
          return { target, method: "setContainer", args: [key, childType] };
        }
        return { target, method: "set", args: [key, scalar(b)] };
      }
      case "List":
      case "MovableList": {
        const list = container as unknown as LoroMovableList;
        const movable = container.kind() === "MovableList";
        const length = list.length;
        const choice = action.op % (movable ? 7 : 5);
        if (length > 0 && (choice === 0 || choice === 1)) {
          const pos = a % length;
          const len = 1 + (b % Math.min(3, length - pos));
          return { target, method: "delete", args: [pos, len] };
        }
        if (choice === 2 && features.nested) {
          return {
            target,
            method: "insertContainer",
            args: [a % (length + 1), childType],
          };
        }
        if (movable && length > 0 && choice === 5) {
          return { target, method: "move", args: [a % length, b % length] };
        }
        if (movable && length > 0 && choice === 6) {
          return { target, method: "set", args: [a % length, scalar(b)] };
        }
        return { target, method: "insert", args: [a % (length + 1), scalar(b)] };
      }
      case "Text": {
        const text = container as unknown as LoroText;
        const length = text.length;
        const choice = action.op % (features.textStyle ? 6 : 4);
        if (length > 0 && choice === 0) {
          const pos = safeTextPos(text, a % length);
          const end = safeTextPos(text, Math.min(length, pos + 1 + (b % 4)));
          return end > pos
            ? { target, method: "delete", args: [pos, end - pos] }
            : undefined;
        }
        if (length > 0 && choice >= 4) {
          const start = safeTextPos(text, a % length);
          const end = safeTextPos(text, Math.min(length, start + 1 + (b % 5)));
          if (end <= start) return undefined;
          const keys = Object.keys(TEXT_STYLES);
          const key = keys[(b >>> 8) % keys.length]!;
          if (choice === 4 || (b & 1) === 0) {
            const value = (b >>> 12) % 3 === 0 ? null : (b >>> 12) % 5;
            return { target, method: "mark", args: [{ start, end }, key, value] };
          }
          return { target, method: "unmark", args: [{ start, end }, key] };
        }
        return {
          target,
          method: "insert",
          args: [safeTextPos(text, a % (length + 1)), textPiece(b)],
        };
      }
      case "Counter":
        return { target, method: "increment", args: [(a % 9) - 4 || 5] };
      case "Tree": {
        const tree = container as unknown as LoroTree;
        const nodes = tree.getNodes().sort((x, y) => (x.id < y.id ? -1 : 1));
        const choice = action.op % 6;
        const childCount = (parent: TreeID | undefined): number =>
          parent === undefined
            ? tree.roots().length
            : tree.getNodeByID(parent)!.children().length;
        if (nodes.length > 0 && choice === 0) {
          return { target, method: "delete", args: [nodes[a % nodes.length]!.id] };
        }
        const parentIndex = b % (nodes.length + 1);
        const parent = parentIndex === nodes.length ? undefined : nodes[parentIndex]!.id;
        if (nodes.length > 0 && (choice === 1 || choice === 2)) {
          const node = nodes[a % nodes.length]!.id;
          const siblings = childCount(parent);
          const current = tree.getNodeByID(node)!;
          const sameParent = current.parent()?.id === parent;
          const position = (b >>> 8) % (sameParent ? siblings : siblings + 1);
          return { target, method: "move", args: [node, parent, position] };
        }
        return {
          target,
          method: "createNode",
          args: [parent, (a >>> 4) % (childCount(parent) + 1)],
        };
      }
    }
    return undefined;
  }

  applyOp(index: number, action: Extract<Action, { kind: "op" }>): void {
    const peer = this.peers[action.peer]!;
    if (peer.doc.isDetached()) {
      this.log(`${peer.name}.attach();`);
      peer.doc.attach();
      peer.rust?.doc.attach();
      this.checkPeer(peer, `attach before action ${index}`);
    }
    const op = this.concreteOp(peer.doc, action);
    if (op !== undefined) {
      this.log(renderOp(peer.name, op));
      const error = executeOp(peer.doc, op, newDetached);
      if (error !== undefined) this.log(`// throws: ${error}`);
      if (peer.rust !== undefined) {
        const rustError = executeOp(peer.rust.doc, op, (type) =>
          this.rust!.newContainer(type),
        );
        if ((error === undefined) !== (rustError === undefined)) {
          throw new FuzzMismatch(
            "lockstep-throws",
            `${op.method}`,
            `${renderOp(peer.name, op)} ${error === undefined ? "succeeds" : `throws "${error}"`} in loro-js but ${rustError === undefined ? "succeeds" : `throws "${rustError}"`} in Rust`,
          );
        }
      }
    }
    if (action.commit) {
      this.log(`${peer.name}.commit();`);
      peer.doc.commit();
      peer.rust?.doc.commit();
      this.checkPeer(peer, `commit after action ${index}`);
    }
  }

  // -- synchronization -----------------------------------------------------

  /** ID spans of every change in `source` that is not covered by `known`. */
  missingSpans(
    source: LoroDoc,
    known: ReturnType<LoroDoc["oplogVersion"]>,
  ): { id: OpId; len: number }[] {
    const spans: { id: OpId; len: number }[] = [];
    for (const [peer, list] of sortedChanges(source)) {
      const seen = known.get(peer) ?? 0;
      for (const change of list) {
        const start = Math.max(change.counter, seen);
        const end = change.counter + change.length;
        if (start < end) {
          spans.push({
            id: { peer: peer as OpId["peer"], counter: start },
            len: end - start,
          });
        }
      }
    }
    return spans;
  }

  sync(index: number, action: Extract<Action, { kind: "sync" }>): void {
    const from = this.peers[action.from]!;
    const to = this.peers[action.to]!;
    for (const peer of [from, to]) {
      peer.doc.commit();
      peer.rust?.doc.commit();
    }
    const rustFrom = from.rust?.doc;
    const rustTo = to.rust?.doc;
    switch (action.mode) {
      case "update":
        this.log(
          `${to.name}.import(${from.name}.export({ mode: "update", from: ${to.name}.oplogVersion() }));`,
        );
        to.doc.import(from.doc.export({ mode: "update", from: to.doc.oplogVersion() }));
        rustTo?.import(rustFrom!.export({ mode: "update", from: rustTo.oplogVersion() }));
        break;
      case "full":
        this.log(`${to.name}.import(${from.name}.export({ mode: "update" }));`);
        to.doc.import(from.doc.export({ mode: "update" }));
        rustTo?.import(rustFrom!.export({ mode: "update" }));
        break;
      case "snapshot":
        this.log(`${to.name}.import(${from.name}.export({ mode: "snapshot" }));`);
        to.doc.import(from.doc.export({ mode: "snapshot" }));
        rustTo?.import(rustFrom!.export({ mode: "snapshot" }));
        break;
      case "shuffled":
      case "batch": {
        const spans = this.missingSpans(from.doc, to.doc.oplogVersion());
        shuffle(spans, new Random(action.r));
        const exportSpan = (span: { id: OpId; len: number }): string =>
          `${from.name}.export({ mode: "updates-in-range", spans: [${JSON.stringify(span)}] })`;
        if (action.mode === "batch") {
          this.log(`${to.name}.importBatch([${spans.map(exportSpan).join(", ")}]);`);
        } else {
          for (const span of spans) this.log(`${to.name}.import(${exportSpan(span)});`);
        }
        const blobs = (doc: { export(mode: unknown): Uint8Array }): Uint8Array[] =>
          spans.map((span) => doc.export({ mode: "updates-in-range", spans: [span] }));
        if (action.mode === "batch") {
          to.doc.importBatch(blobs(from.doc));
          rustTo?.importBatch(blobs(rustFrom!));
        } else {
          for (const blob of blobs(from.doc)) to.doc.import(blob);
          if (rustTo !== undefined)
            for (const blob of blobs(rustFrom!)) rustTo.import(blob);
        }
        break;
      }
    }
    this.checkPeer(to, `import in action ${index}`);
  }

  // -- versions ------------------------------------------------------------

  /** Every operation ID in `doc`'s history, sorted deterministically. */
  allOpIds(doc: LoroDoc): OpId[] {
    const ids: OpId[] = [];
    for (const [peer, list] of sortedChanges(doc)) {
      for (const change of list) {
        for (let offset = 0; offset < change.length; offset += 1) {
          ids.push({ peer: peer as OpId["peer"], counter: change.counter + offset });
        }
      }
    }
    return ids;
  }

  pickFrontiers(doc: LoroDoc, r: number, r2: number): Frontiers | undefined {
    const ids = this.allOpIds(doc);
    if (ids.length === 0) return undefined;
    const first = ids[r % ids.length]!;
    if (r2 % 4 !== 0) return [first];
    const second = ids[r2 % ids.length]!;
    const version = doc.frontiersToVV([first]);
    version.merge(doc.frontiersToVV([second]));
    return doc.vvToFrontiers(version);
  }

  /** A fresh document containing exactly the changes included in `frontiers`. */
  replayAt(source: LoroDoc, frontiers: Frontiers): LoroDoc {
    const version = source.frontiersToVV(frontiers);
    const spans = [...version.toJSON().entries()]
      .filter(([, end]) => end > 0)
      .map(([peer, end]) => ({ id: { peer, counter: 0 }, len: end }));
    const doc = this.newDoc();
    if (spans.length > 0) doc.import(source.export({ mode: "updates-in-range", spans }));
    return doc;
  }

  checkoutAction(index: number, action: Extract<Action, { kind: "checkout" }>): void {
    const peer = this.peers[action.peer]!;
    peer.doc.commit();
    peer.rust?.doc.commit();
    const frontiers = this.pickFrontiers(peer.doc, action.r, action.r2);
    if (frontiers === undefined) return;
    this.log(`${peer.name}.checkout(${frontiersLiteral(frontiers)});`);
    peer.doc.checkout(frontiers);
    peer.rust?.doc.checkout(frontiers);
    this.checkPeer(peer, `checkout in action ${index}`);
    if (this.checks.checkout) {
      assertSameValue(
        "checkout-vs-replay",
        this.value(peer.doc),
        this.value(this.replayAt(peer.doc, frontiers)),
        ` (${peer.name} at ${frontiersLiteral(frontiers)})`,
      );
    }
    if (!action.stay) {
      this.log(`${peer.name}.checkoutToLatest();`);
      peer.doc.checkoutToLatest();
      peer.rust?.doc.checkoutToLatest();
      this.checkPeer(peer, `checkoutToLatest in action ${index}`);
    }
  }

  // -- final checks ----------------------------------------------------------

  finish(): void {
    for (const peer of this.peers) {
      peer.doc.commit();
      peer.rust?.doc.commit();
      if (peer.doc.isDetached()) {
        this.log(`${peer.name}.checkoutToLatest();`);
        peer.doc.checkoutToLatest();
        peer.rust?.doc.checkoutToLatest();
        this.checkPeer(peer, "final attach");
      }
    }
    this.log("// final: every peer imports every other peer's updates");
    for (let round = 0; round < 2; round += 1) {
      for (const from of this.peers) {
        for (const to of this.peers) {
          if (from === to) continue;
          to.doc.import(from.doc.export({ mode: "update", from: to.doc.oplogVersion() }));
          to.rust?.doc.import(
            from.rust!.doc.export({ mode: "update", from: to.rust.doc.oplogVersion() }),
          );
        }
      }
    }
    for (const peer of this.peers) this.checkPeer(peer, `final sync of ${peer.name}`);
    const base = this.peers[0]!.doc;
    const baseValue = this.value(base);
    for (const peer of this.peers.slice(1)) {
      assertSameValue(
        "peer-divergence",
        this.value(peer.doc),
        baseValue,
        ` (${peer.name} vs p1)`,
      );
    }

    const updates = base.export({ mode: "update" });
    const replay: Mirrored<LoroDoc> = {
      doc: this.newDoc(),
      mirror: new EventMirror(),
      mirrorError: undefined,
    };
    if (this.checks.events) this.subscribe(replay);
    replay.doc.import(updates);
    this.checkMirror(replay, "replay import");
    const replayValue = this.value(replay.doc);
    const rust = this.newRustDoc();
    if (rust !== undefined) {
      if (this.checks.events) {
        this.rustReplay = {
          doc: rust,
          mirror: new EventMirror(),
          mirrorError: undefined,
        };
        this.subscribe(this.rustReplay);
      }
      rust.import(updates);
      this.rust!.flushEvents();
      assertSameValue(
        "rust-replay",
        replayValue,
        this.rustValue(rust),
        " (loro-js replay vs Rust replay)",
      );
    }
    assertSameValue(
      "replay-vs-peers",
      baseValue,
      replayValue,
      " (p1 vs one-shot replay)",
    );

    const shuffled = this.newDoc();
    const spans = this.missingSpans(base, shuffled.oplogVersion());
    shuffle(spans, new Random(this.scenario.seed ^ 0x5bd1_e995));
    for (const span of spans) {
      shuffled.import(base.export({ mode: "updates-in-range", spans: [span] }));
    }
    assertSameValue(
      "shuffled-replay",
      this.value(shuffled),
      replayValue,
      " (per-change shuffled replay vs one-shot replay)",
    );

    const versions = this.checkoutVersions(base);
    if (this.checks.checkout) {
      this.checkoutEverywhere("replay", replay, versions, rust);
      this.checkoutEverywhere("p1", this.peers[0]!, versions, undefined);
    }
    if (this.checks.snapshot) {
      // Events are not mirrored here: the mirror would need the imported state as its base.
      const snapshot: Mirrored<LoroDoc> = {
        doc: this.newDoc(),
        mirror: new EventMirror(),
        mirrorError: undefined,
      };
      snapshot.doc.import(base.export({ mode: "snapshot" }));
      assertSameValue(
        "snapshot-latest",
        this.value(snapshot.doc),
        replayValue,
        " (snapshot import)",
      );
      if (this.checks.checkout) {
        this.checkoutEverywhere("snapshot", snapshot, versions, undefined, false);
      }
    }
    if (this.checks.shallow) this.checkShallow(base, versions);
  }

  checkoutVersions(doc: LoroDoc): Frontiers[] {
    const ids = this.allOpIds(doc);
    const limit = this.options.maxCheckoutVersions ?? 48;
    const random = new Random(this.scenario.seed ^ 0x2545_f491);
    const versions: Frontiers[] = [];
    if (ids.length <= limit) {
      for (const id of ids) versions.push([id]);
    } else {
      for (let index = 0; index < limit; index += 1) {
        versions.push([ids[random.below(ids.length)]!]);
      }
    }
    for (let index = 0; index < Math.min(8, ids.length); index += 1) {
      const frontiers = this.pickFrontiers(doc, random.next(), 0);
      if (frontiers !== undefined) versions.push(frontiers);
    }
    return versions;
  }

  checkoutEverywhere(
    label: string,
    target: Mirrored<LoroDoc>,
    versions: readonly Frontiers[],
    rust: RustDoc | undefined,
    mirrored = true,
  ): void {
    const base = this.peers[0]!.doc;
    const order = [...versions];
    shuffle(order, new Random(this.scenario.seed ^ 0x68e3_1da4));
    for (const frontiers of order) {
      const context = ` (${label}.checkout(${frontiersLiteral(frontiers)}))`;
      this.log(`${label}.checkout(${frontiersLiteral(frontiers)});`);
      try {
        target.doc.checkout(frontiers);
      } catch (error) {
        const message = String((error as Error).message);
        throw new FuzzMismatch(
          `checkout-throws:${label}`,
          normalizeMessage(message),
          `${label}.checkout(${frontiersLiteral(frontiers)}) threw: ${message}`,
        );
      }
      if (mirrored)
        this.checkMirror(target, `${label} checkout ${frontiersLiteral(frontiers)}`);
      const expected = this.value(this.replayAt(base, frontiers));
      assertSameValue(`checkout:${label}`, this.value(target.doc), expected, context);
      if (rust !== undefined) {
        rust.checkout(frontiers);
        assertSameValue("rust-checkout", expected, this.rustValue(rust), context);
        if (this.rustReplay?.doc === rust) {
          this.rust!.flushEvents();
          this.checkMirror(this.rustReplay, `Rust ${label} checkout`, "rust-event");
        }
      }
    }
    target.doc.checkoutToLatest();
    if (mirrored) this.checkMirror(target, `${label} checkoutToLatest`);
    rust?.checkoutToLatest();
  }

  checkShallow(base: LoroDoc, versions: readonly Frontiers[]): void {
    const random = new Random(this.scenario.seed ^ 0x1b87_3593);
    if (this.allOpIds(base).length === 0) return;
    const latest = this.value(base);
    const rustFull = this.newRustDoc();
    rustFull?.import(base.export({ mode: "update" }));
    for (let attempt = 0; attempt < 3; attempt += 1) {
      const root = this.pickFrontiers(base, random.next(), random.next())!;
      const context = ` (shallow at ${frontiersLiteral(root)})`;
      this.log(
        `const shallow = LoroDoc.fromSnapshot(p1.export({ mode: "shallow-snapshot", frontiers: ${frontiersLiteral(root)} }));`,
      );
      const blob = base.export({ mode: "shallow-snapshot", frontiers: root });
      const shallow = this.newDoc();
      shallow.import(blob);
      assertSameValue("shallow-latest", this.value(shallow), latest, context);
      if (rustFull !== undefined) {
        const rustShallow = this.newRustDoc()!;
        rustShallow.import(blob);
        const rustRoot = this.newRustDoc()!;
        rustRoot.import(rustFull.export({ mode: "shallow-snapshot", frontiers: root }));
        const roots = [shallow, rustRoot].map((doc) =>
          frontiersLiteral(
            [...doc.shallowSinceFrontiers()].sort((a, b) =>
              a.peer === b.peer ? a.counter - b.counter : a.peer < b.peer ? -1 : 1,
            ),
          ),
        );
        if (roots[0] !== roots[1]) {
          throw new FuzzMismatch(
            "shallow-root",
            "root",
            `shallow root${context}: loro-js ${roots[0]}, Rust ${roots[1]}`,
          );
        }
        assertSameValue(
          "rust-imports-ts-shallow",
          this.rustValue(rustShallow),
          latest,
          context,
        );
        const fromRust = this.newDoc();
        fromRust.import(rustFull.export({ mode: "shallow-snapshot", frontiers: root }));
        assertSameValue("ts-imports-rust-shallow", this.value(fromRust), latest, context);
      }
      if (!this.checks.checkout) continue;
      const since = base.frontiersToVV(shallow.shallowSinceFrontiers());
      for (const frontiers of versions) {
        const order = since.compare(base.frontiersToVV(frontiers));
        if (order !== -1 && order !== 0) continue;
        this.log(`shallow.checkout(${frontiersLiteral(frontiers)});`);
        let actual: NormalizedValue;
        try {
          shallow.checkout(frontiers);
          actual = this.value(shallow);
        } catch (error) {
          const message = String((error as Error).message);
          throw new FuzzMismatch(
            "shallow-checkout-throws",
            normalizeMessage(message),
            `shallow.checkout(${frontiersLiteral(frontiers)}) threw${context}: ${message}`,
          );
        }
        assertSameValue(
          "shallow-checkout",
          actual,
          this.value(this.replayAt(base, frontiers)),
          `${context} checkout ${frontiersLiteral(frontiers)}`,
        );
      }
      shallow.checkoutToLatest();
      assertSameValue(
        "shallow-latest",
        this.value(shallow),
        latest,
        `${context} after checkouts`,
      );
    }
  }
}

function safeTextPos(text: LoroText, pos: number): number {
  // Never split a surrogate pair.
  const value = text.toString();
  let clamped = Math.min(pos, value.length);
  const code = value.charCodeAt(clamped - 1);
  if (clamped > 0 && code >= 0xd800 && code <= 0xdbff) clamped -= 1;
  return clamped;
}

function shuffle<T>(items: T[], random: Random): void {
  for (let index = items.length - 1; index > 0; index -= 1) {
    const other = random.below(index + 1);
    [items[index], items[other]] = [items[other]!, items[index]!];
  }
}

function normalizeMessage(message: string): string {
  return message
    .split("\n")[0]!
    .replace(/cid:[^\s)"]+/g, "CID")
    .replace(/\d+/g, "N")
    .slice(0, 120);
}

/** Runs a scenario; returns undefined on success or the first failure. */
export function runScenario(
  scenario: Scenario,
  options: RunOptions = {},
): FuzzFailure | undefined {
  const runner = new Runner(scenario, options);
  let phase = "setup";
  try {
    for (const [index, action] of scenario.actions.entries()) {
      phase = `${action.kind}#${index}`;
      if (action.kind === "op") runner.applyOp(index, action);
      else if (action.kind === "sync") runner.sync(index, action);
      else runner.checkoutAction(index, action);
    }
    phase = "final";
    runner.finish();
    return undefined;
  } catch (error) {
    if (error instanceof FuzzMismatch) {
      return {
        check: error.check,
        signature: `${error.check}:${error.key}`,
        message: error.message,
        trace: runner.trace,
      };
    }
    const message = (error as Error)?.stack ?? String(error);
    const kind = phase.replace(/#\d+$/, "");
    return {
      check: `throws:${kind}`,
      signature: `throws:${kind}:${normalizeMessage((error as Error)?.message ?? String(error))}`,
      message: `${phase} threw: ${message}`,
      trace: runner.trace,
    };
  }
}

/**
 * Delta-debugging over the action list. Keeps a reduction only when the
 * failure keeps the same signature.
 */
export function minimizeScenario(
  scenario: Scenario,
  signature: string,
  options: RunOptions = {},
  budget = 4_000,
): Scenario {
  let current = scenario;
  let runs = 0;
  const fails = (candidate: Scenario): boolean => {
    runs += 1;
    return (
      runScenario(candidate, { ...options, onTrace: undefined })?.signature === signature
    );
  };
  let chunk = Math.max(1, Math.floor(current.actions.length / 2));
  while (chunk >= 1 && runs < budget) {
    let removed = false;
    for (let start = 0; start < current.actions.length && runs < budget; ) {
      const actions = [
        ...current.actions.slice(0, start),
        ...current.actions.slice(start + chunk),
      ];
      const candidate = { ...current, actions };
      if (actions.length < current.actions.length && fails(candidate)) {
        current = candidate;
        removed = true;
      } else {
        start += chunk;
      }
    }
    if (!removed) chunk = Math.floor(chunk / 2);
  }
  // Fewer peers make the reproduction easier to read.
  for (let peers = 2; peers < current.peers && runs < budget; peers += 1) {
    const actions = current.actions.filter((action) =>
      action.kind === "sync"
        ? action.from < peers && action.to < peers
        : action.peer < peers,
    );
    const candidate = { ...current, peers, actions };
    if (fails(candidate)) {
      current = candidate;
      break;
    }
  }
  return current;
}

export function describeScenario(scenario: Scenario): string {
  return JSON.stringify({
    seed: scenario.seed,
    peers: scenario.peers,
    features: scenario.features,
    actions: scenario.actions,
  });
}
