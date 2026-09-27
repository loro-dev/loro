import { writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { isDeepStrictEqual } from "node:util";

import type { ContainerLike, DocLike, EngineModule, FrontiersLike } from "./engine";
import {
  ENGINES,
  Divergence,
  Rng,
  Twin,
  assertTwinAgrees,
  canonicalDiff,
  describeError,
  mapContainer,
  movableList,
  plain,
  reachableContainers,
  sortedFrontiers,
  stringify,
  textContainer,
  vvToMap,
  type EngineName,
} from "./harness";

export type ChildKind = "Map" | "Text" | "MovableList";
export type SyncMode = "update" | "range" | "snapshot" | "batch";

export type Action =
  | {
      readonly kind: "insert";
      readonly peer: number;
      readonly target: string;
      readonly pos: number;
      readonly value: string | number;
    }
  | {
      readonly kind: "insertContainer";
      readonly peer: number;
      readonly target: string;
      readonly pos: number;
      readonly child: ChildKind;
    }
  | {
      readonly kind: "delete";
      readonly peer: number;
      readonly target: string;
      readonly pos: number;
      readonly len: number;
    }
  | {
      readonly kind: "move";
      readonly peer: number;
      readonly target: string;
      readonly from: number;
      readonly to: number;
    }
  | {
      readonly kind: "set";
      readonly peer: number;
      readonly target: string;
      readonly pos: number;
      readonly value: string | number;
    }
  | {
      readonly kind: "setContainer";
      readonly peer: number;
      readonly target: string;
      readonly pos: number;
      readonly child: ChildKind;
    }
  | {
      readonly kind: "mapSet";
      readonly peer: number;
      readonly target: string;
      readonly key: string;
      readonly value: string | number;
    }
  | {
      readonly kind: "textInsert";
      readonly peer: number;
      readonly target: string;
      readonly pos: number;
      readonly text: string;
    }
  | { readonly kind: "commit"; readonly peer: number }
  | {
      readonly kind: "sync";
      readonly from: number;
      readonly to: number;
      readonly mode: SyncMode;
      readonly cross: boolean;
      readonly via?: number;
    }
  | {
      readonly kind: "join";
      readonly from: number;
      readonly peer: number;
      readonly shallow: boolean;
      readonly at: number;
      readonly cross: boolean;
    }
  | { readonly kind: "checkout"; readonly peer: number; readonly at: number }
  | { readonly kind: "revert"; readonly peer: number; readonly at: number }
  | {
      readonly kind: "diff";
      readonly peer: number;
      readonly from: number;
      readonly to: number;
    }
  | { readonly kind: "undo"; readonly peer: number }
  | { readonly kind: "redo"; readonly peer: number };

export interface FuzzProfile {
  readonly name: string;
  readonly peers: number;
  /** Allow move/set/setContainer on movable lists. */
  readonly moves: boolean;
  /** Allow nested child containers. */
  readonly children: boolean;
  readonly undo: boolean;
  readonly history: boolean;
  readonly snapshots: boolean;
  readonly shallow: boolean;
  readonly crossEngine: boolean;
  readonly metadata: boolean;
  readonly events: boolean;
}

export const ROOT_LISTS = ["cid:root-list:MovableList", "cid:root-list2:MovableList"];

export class FuzzRun {
  readonly rng: Rng;
  readonly twins = new Map<number, Twin>();
  readonly log: Action[] = [];
  #nextValue = 0;
  /** Last action kind per peer, to keep undo/redo unchained (see `generate`). */
  readonly #lastKind = new Map<number, Action["kind"]>();

  constructor(
    readonly engines: Record<EngineName, EngineModule>,
    readonly profile: FuzzProfile,
    readonly seed: number,
  ) {
    this.rng = new Rng(seed);
    for (let peer = 1; peer <= profile.peers; peer += 1) {
      this.twins.set(
        peer,
        new Twin(engines, `peer${peer}`, peer, undefined, { undo: profile.undo }),
      );
    }
  }

  run(steps: number): void {
    for (let step = 0; step < steps; step += 1) {
      const action = this.generate();
      this.log.push(action);
      try {
        this.apply(action);
        this.compareAll(`after step ${step} (${action.kind})`);
      } catch (error) {
        throw new Divergence(
          `seed ${this.seed} profile ${this.profile.name} step ${step}: ${describeError(error)}\n` +
            `actions:\n${this.log.map((item) => `  ${stringify(item)}`).join("\n")}`,
        );
      }
    }
    for (const action of this.finalSync()) {
      this.log.push(action);
      try {
        this.apply(action);
        this.compareAll(`after final ${action.kind}`);
      } catch (error) {
        throw new Divergence(
          `seed ${this.seed} profile ${this.profile.name} final sync: ${describeError(error)}\n` +
            `actions:\n${this.log.map((item) => `  ${stringify(item)}`).join("\n")}`,
        );
      }
    }
    this.assertConverged();
  }

  replay(actions: readonly Action[]): void {
    for (const [step, action] of actions.entries()) {
      this.log.push(action);
      this.apply(action);
      this.compareAll(`after replayed step ${step} (${action.kind})`);
    }
  }

  compareAll(context: string): void {
    for (const twin of this.twins.values()) {
      assertTwinAgrees(twin, context, {
        metadata: this.profile.metadata,
        events: this.profile.events,
      });
    }
  }

  /** Full-history peers that exchanged everything must hold the same state. */
  assertConverged(): void {
    const full = [...this.twins.values()].filter((twin) => !twin.docs.rust.isShallow());
    const first = full[0];
    if (first === undefined) return;
    const expected = stringify(plain(first.docs.rust.toJSON()));
    for (const twin of full.slice(1)) {
      const version = vvToMap(twin.docs.rust.oplogVersion());
      if (!isDeepStrictEqual(version, vvToMap(first.docs.rust.oplogVersion()))) continue;
      const actual = stringify(plain(twin.docs.rust.toJSON()));
      if (actual !== expected) {
        throw new Divergence(
          `seed ${this.seed}: Rust peers with equal versions diverged (${first.name} vs ${twin.name})`,
        );
      }
    }
  }

  *finalSync(): Generator<Action> {
    const peers = [...this.twins.keys()].filter(
      (peer) => !this.twins.get(peer)!.docs.rust.isShallow(),
    );
    for (const from of peers) {
      for (const to of peers) {
        if (from === to) continue;
        yield { kind: "sync", from, to, mode: "update", cross: this.profile.crossEngine };
      }
    }
  }

  generate(): Action {
    const action = this.generateAction();
    this.#lastKind.set("peer" in action ? action.peer : action.to, action.kind);
    return action;
  }

  generateAction(): Action {
    const rng = this.rng;
    const peers = [...this.twins.keys()];
    const peer = rng.pick(peers);
    const twin = this.twins.get(peer)!;
    const profile = this.profile;
    // loro.js undo replays ops while Rust applies transformed inverse diffs, so
    // an undo whose target was recreated by an earlier undo diverges for List
    // too. Undo only right after an edit, and redo only right after an undo.
    const last = this.#lastKind.get(peer);
    const canUndo = profile.undo && last !== "undo" && last !== "redo";
    const canRedo = profile.undo && last === "undo";
    const kind = rng.weighted<Action["kind"] | "edit">({
      edit: 12,
      commit: 3,
      ...(peers.length > 1 ? { sync: 4 } : {}),
      ...(canUndo ? { undo: 2 } : {}),
      ...(canRedo ? { redo: 2 } : {}),
      ...(profile.history ? { checkout: 0.8, revert: 0.4, diff: 0.6 } : {}),
      ...(profile.snapshots && peers.length > 1 && this.twins.size < 5
        ? { join: 0.3 }
        : {}),
    });
    switch (kind) {
      case "edit":
        return this.generateEdit(peer, twin);
      case "commit":
      case "undo":
      case "redo":
        return { kind, peer };
      case "sync": {
        const from = rng.pick(peers.filter((item) => item !== peer)) ?? peer;
        const mode = rng.weighted<SyncMode>({
          update: 4,
          range: 1,
          ...(profile.snapshots ? { snapshot: 1 } : {}),
          batch: 1,
        });
        const others = peers.filter((item) => item !== peer && item !== from);
        return {
          kind: "sync",
          from,
          to: peer,
          mode,
          cross: profile.crossEngine && rng.bool(0.6),
          ...(mode === "batch" && others.length > 0 ? { via: rng.pick(others) } : {}),
        };
      }
      case "join": {
        const joined = Math.max(...peers) + 1;
        return {
          kind: "join",
          from: peer,
          peer: joined,
          shallow: profile.shallow && rng.bool(0.5),
          at: twin.history.length === 0 ? -1 : rng.int(twin.history.length + 1) - 1,
          cross: profile.crossEngine && rng.bool(0.6),
        };
      }
      case "checkout":
      case "revert":
        return {
          kind,
          peer,
          at: twin.history.length === 0 ? -1 : rng.int(twin.history.length),
        };
      case "diff":
        return {
          kind: "diff",
          peer,
          from: twin.history.length === 0 ? -1 : rng.int(twin.history.length + 1) - 1,
          to: twin.history.length === 0 ? -1 : rng.int(twin.history.length + 1) - 1,
        };
      default:
        throw new Error(`unexpected action ${kind}`);
    }
  }

  generateEdit(peer: number, twin: Twin): Action {
    const rng = this.rng;
    const doc = twin.docs.rust;
    const containers = reachableContainers(doc).filter(
      ({ id }) => !ROOT_LISTS.includes(id),
    );
    const lists = [
      ...ROOT_LISTS,
      ...containers.filter((c) => c.kind === "MovableList").map((c) => c.id),
    ];
    const maps = containers.filter((c) => c.kind === "Map").map((c) => c.id);
    const texts = containers.filter((c) => c.kind === "Text").map((c) => c.id);
    const targetKind = rng.weighted({
      list: 10,
      ...(maps.length > 0 ? { map: 1 } : {}),
      ...(texts.length > 0 ? { text: 1 } : {}),
    });
    if (targetKind === "map") {
      return {
        kind: "mapSet",
        peer,
        target: rng.pick(maps),
        key: `k${rng.int(3)}`,
        value: this.value(),
      };
    }
    if (targetKind === "text") {
      const target = rng.pick(texts);
      const length = textContainer(doc, target).length;
      return {
        kind: "textInsert",
        peer,
        target,
        pos: rng.int(length + 1),
        text: `t${this.#nextValue++}`,
      };
    }
    const target = rng.pick(lists);
    const length = movableList(doc, target).length;
    const profile = this.profile;
    const kind = rng.weighted<Action["kind"]>({
      insert: 4,
      ...(profile.children ? { insertContainer: 0.6 } : {}),
      ...(length > 0 ? { delete: 2.5 } : {}),
      ...(profile.moves && length > 1 ? { move: 4 } : {}),
      ...(profile.moves && length > 0 ? { set: 2.5 } : {}),
      ...(profile.moves && profile.children && length > 0 ? { setContainer: 0.3 } : {}),
    });
    switch (kind) {
      case "insert":
        return { kind, peer, target, pos: rng.int(length + 1), value: this.value() };
      case "insertContainer":
        return { kind, peer, target, pos: rng.int(length + 1), child: this.child() };
      case "delete": {
        const pos = rng.int(length);
        return { kind, peer, target, pos, len: 1 + rng.int(Math.min(3, length - pos)) };
      }
      case "move":
        return { kind, peer, target, from: rng.int(length), to: rng.int(length) };
      case "set":
        return { kind, peer, target, pos: rng.int(length), value: this.value() };
      case "setContainer":
        return { kind, peer, target, pos: rng.int(length), child: this.child() };
      default:
        throw new Error(`unexpected edit ${kind}`);
    }
  }

  value(): string | number {
    const value = this.#nextValue++;
    return this.rng.bool(0.3) ? value : `v${value}`;
  }

  child(): ChildKind {
    return this.rng.pick<ChildKind>(["Map", "Text", "MovableList"]);
  }

  apply(action: Action): void {
    switch (action.kind) {
      case "insert":
      case "insertContainer":
      case "delete":
      case "move":
      case "set":
      case "setContainer":
      case "mapSet":
      case "textInsert":
        this.applyEdit(action);
        return;
      case "commit":
        this.twin(action.peer).both("commit", (doc) => doc.commit());
        this.twin(action.peer).record();
        return;
      case "undo":
      case "redo": {
        const twin = this.twin(action.peer);
        const managers = twin.undo;
        if (managers === undefined) return;
        // loro.js picks the undo item before committing the pending transaction,
        // Rust commits first. That is unrelated to MovableList; commit up front.
        twin.both("commit before undo", (doc) => doc.commit());
        assertTwinAgrees(twin, "after commit before undo", {
          metadata: this.profile.metadata,
          events: this.profile.events,
        });
        const results = twin.both(action.kind, (_doc, engine) =>
          managers[engine][action.kind](),
        );
        if (results !== undefined && results.rust !== results.js) {
          throw new Divergence(
            `${twin.name}: ${action.kind} returned ${results.rust} in Rust but ${results.js} in loro.js`,
          );
        }
        twin.record();
        return;
      }
      case "sync":
        this.applySync(action);
        return;
      case "join":
        this.applyJoin(action);
        return;
      case "checkout": {
        const twin = this.twin(action.peer);
        const frontiers = this.historyAt(twin, action.at);
        // loro.js stays detached after checking out the latest frontiers while
        // Rust reattaches; that is unrelated to MovableList, so skip it.
        twin.both("commit before checkout", (doc) => doc.commit());
        assertTwinAgrees(twin, "after commit before checkout", {
          metadata: this.profile.metadata,
          events: this.profile.events,
        });
        if (
          isDeepStrictEqual(
            sortedFrontiers(frontiers),
            sortedFrontiers(twin.docs.rust.frontiers()),
          )
        ) {
          return;
        }
        twin.both("checkout", (doc) => doc.checkout(frontiers));
        assertTwinAgrees(twin, `at checkout ${stringify(frontiers)}`, {
          metadata: this.profile.metadata,
          events: this.profile.events,
        });
        twin.both("checkoutToLatest", (doc) => doc.checkoutToLatest());
        return;
      }
      case "revert": {
        const twin = this.twin(action.peer);
        const frontiers = this.historyAt(twin, action.at);
        twin.both("revertTo", (doc) => {
          doc.revertTo(frontiers);
          doc.commit();
        });
        twin.record();
        return;
      }
      case "diff": {
        const twin = this.twin(action.peer);
        const from = this.historyAt(twin, action.from);
        const to = this.historyAt(twin, action.to);
        // loro.js also reports containers that are dead at `to`; Rust does not.
        const reachable = reachableAt(twin.docs.rust, to);
        const results = twin.both("diff", (doc) =>
          doc
            .diff(from, to, false)
            .filter(([id]) => reachable === undefined || reachable.has(id))
            .map(([id, diff]) => [id, canonicalDiff(diff)] as const)
            .sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0)),
        );
        if (results !== undefined && !isDeepStrictEqual(results.rust, results.js)) {
          throw new Divergence(
            `${twin.name}: diff(${stringify(from)}, ${stringify(to)}) differs\n` +
              `  rust: ${stringify(results.rust)}\n  js:   ${stringify(results.js)}`,
          );
        }
        return;
      }
    }
  }

  twin(peer: number): Twin {
    return this.twins.get(peer)!;
  }

  historyAt(twin: Twin, index: number): FrontiersLike {
    return index < 0 ? [] : (twin.history[index] ?? []);
  }

  applyEdit(action: Extract<Action, { target: string }>): void {
    const twin = this.twin(action.peer);
    twin.both(action.kind, (doc, engine) => {
      const engineModule = this.engines[engine];
      switch (action.kind) {
        case "insert":
          movableList(doc, action.target).insert(action.pos, action.value);
          return;
        case "insertContainer":
          movableList(doc, action.target).insertContainer(
            action.pos,
            newChild(engineModule, action.child),
          );
          return;
        case "delete":
          movableList(doc, action.target).delete(action.pos, action.len);
          return;
        case "move":
          movableList(doc, action.target).move(action.from, action.to);
          return;
        case "set":
          movableList(doc, action.target).set(action.pos, action.value);
          return;
        case "setContainer":
          movableList(doc, action.target).setContainer(
            action.pos,
            newChild(engineModule, action.child),
          );
          return;
        case "mapSet":
          mapContainer(doc, action.target).set(action.key, action.value);
          return;
        case "textInsert":
          textContainer(doc, action.target).insert(action.pos, action.text);
          return;
      }
    });
  }

  applySync(action: Extract<Action, { kind: "sync" }>): void {
    const source = this.twin(action.from);
    const target = this.twin(action.to);
    const via = action.via === undefined ? undefined : this.twin(action.via);
    // Rust's oplog version counts a pending transaction and loro.js's does not;
    // commit first so both halves export the same span.
    for (const twin of [source, target, via]) {
      if (twin === undefined) continue;
      twin.both("commit before sync", (doc) => doc.commit());
      assertTwinAgrees(twin, "after commit before sync", {
        metadata: this.profile.metadata,
        events: this.profile.events,
      });
    }
    const exported = (engine: EngineName): Uint8Array[] => {
      const sourceEngine: EngineName = action.cross ? other(engine) : engine;
      const doc = source.docs[sourceEngine];
      const blobs = [this.exportFor(doc, target.docs[engine], sourceEngine, action.mode)];
      if (via !== undefined) {
        blobs.push(
          this.exportFor(
            via.docs[sourceEngine],
            target.docs[engine],
            sourceEngine,
            "update",
          ),
        );
      }
      return blobs;
    };
    const blobs: Record<EngineName, Uint8Array[]> = {
      rust: exported("rust"),
      js: exported("js"),
    };
    target.batchImport = action.mode === "batch";
    try {
      target.both(`import from ${source.name} (${action.mode})`, (doc, engine) =>
        action.mode === "batch"
          ? doc.importBatch(blobs[engine])
          : doc.import(blobs[engine][0]!),
      );
    } finally {
      target.batchImport = false;
    }
    target.record();
  }

  exportFor(
    doc: DocLike,
    receiver: DocLike,
    sourceEngine: EngineName,
    mode: SyncMode,
  ): Uint8Array {
    const receiverVersion = vvToMap(receiver.oplogVersion());
    switch (mode) {
      case "snapshot":
        return doc.export({ mode: "snapshot" });
      case "range": {
        const spans = [...vvToMap(doc.oplogVersion())].flatMap(([peer, end]) => {
          const start = receiverVersion.get(peer) ?? 0;
          return end > start ? [{ id: { peer, counter: start }, len: end - start }] : [];
        });
        return doc.export({ mode: "updates-in-range", spans });
      }
      case "update":
      case "batch":
        return doc.export({
          mode: "update",
          from: new this.engines[sourceEngine].VersionVector(receiverVersion),
        });
    }
  }

  applyJoin(action: Extract<Action, { kind: "join" }>): void {
    const source = this.twin(action.from);
    source.both("commit before join", (doc) => doc.commit());
    assertTwinAgrees(source, "after commit before join", {
      metadata: this.profile.metadata,
      events: this.profile.events,
    });
    const frontiers =
      action.at < 0
        ? source.docs.rust.oplogFrontiers()
        : this.historyAt(source, action.at);
    const bytes: Record<EngineName, Uint8Array> = {
      rust: new Uint8Array(),
      js: new Uint8Array(),
    };
    for (const engine of ENGINES) {
      const sourceEngine: EngineName = action.cross ? other(engine) : engine;
      const doc = source.docs[sourceEngine];
      bytes[engine] = action.shallow
        ? doc.export({ mode: "shallow-snapshot", frontiers })
        : doc.export({ mode: "snapshot" });
    }
    const twin = new Twin(this.engines, `peer${action.peer}`, action.peer, undefined, {
      undo: this.profile.undo,
    });
    this.twins.set(action.peer, twin);
    twin.both("import joined snapshot", (doc, engine) => doc.import(bytes[engine]));
    twin.record();
  }
}

/** Container IDs reachable at `frontiers`, or undefined when that cannot be forked. */
function reachableAt(doc: DocLike, frontiers: FrontiersLike): Set<string> | undefined {
  try {
    return new Set(reachableContainers(doc.forkAt(frontiers)).map(({ id }) => id));
  } catch {
    return undefined;
  }
}

function other(engine: EngineName): EngineName {
  return engine === "rust" ? "js" : "rust";
}

function newChild(engine: EngineModule, kind: ChildKind): ContainerLike {
  switch (kind) {
    case "Map":
      return new engine.LoroMap();
    case "Text":
      return new engine.LoroText();
    case "MovableList":
      return new engine.LoroMovableList();
  }
}

export interface SavedTrace {
  readonly profile: FuzzProfile;
  readonly seed: number;
  readonly actions: readonly Action[];
}

/** Replays `trace`, returning the divergence message or undefined when both engines agree. */
export function replayTrace(
  engines: Record<EngineName, EngineModule>,
  trace: SavedTrace,
): string | undefined {
  try {
    new FuzzRun(engines, trace.profile, trace.seed).replay(trace.actions);
    return undefined;
  } catch (error) {
    return error instanceof Divergence
      ? describeError(error)
      : `crash: ${describeError(error)}`;
  }
}

/** Greedily drops actions while the trace still diverges (not merely crashes). */
export function minimizeTrace(
  engines: Record<EngineName, EngineModule>,
  trace: SavedTrace,
): SavedTrace {
  let actions = [...trace.actions];
  let changed = true;
  while (changed) {
    changed = false;
    for (let index = actions.length - 1; index >= 0; index -= 1) {
      const candidate = actions.filter((_, i) => i !== index);
      const message = replayTrace(engines, { ...trace, actions: candidate });
      if (message !== undefined && !message.startsWith("crash")) {
        actions = candidate;
        changed = true;
      }
    }
  }
  return { ...trace, actions };
}

function saveTrace(trace: SavedTrace): string {
  const safeName = trace.profile.name.replaceAll(/[^a-z0-9]+/giu, "-");
  const path = join(tmpdir(), `loro-js-differential-${safeName}-${trace.seed}.json`);
  writeFileSync(path, JSON.stringify(trace));
  return path;
}

export interface FuzzSummary {
  readonly seeds: number;
  readonly failures: { readonly seed: number; readonly message: string }[];
}

export function fuzz(
  engines: Record<EngineName, EngineModule>,
  profile: FuzzProfile,
  seeds: Iterable<number>,
  steps: number,
): FuzzSummary {
  const failures: { seed: number; message: string }[] = [];
  let count = 0;
  for (const seed of seeds) {
    count += 1;
    const run = new FuzzRun(engines, profile, seed);
    try {
      run.run(steps);
    } catch (error) {
      if (!(error instanceof Divergence)) throw error;
      const path = saveTrace({ profile, seed, actions: run.log });
      failures.push({
        seed,
        message: `${error.message}\nTrace saved to ${path}; replay it with LORO_JS_DIFF_REPLAY=${path}`,
      });
    }
  }
  return { seeds: count, failures };
}

export function seedRange(start: number, count: number): number[] {
  return Array.from({ length: count }, (_, index) => start + index);
}

/** Environment overrides for longer local runs. */
export function fuzzScale(
  defaultSeeds: number,
  defaultSteps: number,
): {
  readonly seeds: number[];
  readonly steps: number;
} {
  const start = Number(process.env["LORO_JS_DIFF_SEED"] ?? 1);
  const count = Number(process.env["LORO_JS_DIFF_SEEDS"] ?? defaultSeeds);
  const steps = Number(process.env["LORO_JS_DIFF_STEPS"] ?? defaultSteps);
  return { seeds: seedRange(start, count), steps };
}
