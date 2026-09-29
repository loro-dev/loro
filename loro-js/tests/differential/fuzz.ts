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
  eventShapeDifferences,
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
  /** Peer IDs are never reused, including those of peers that left the run. */
  #nextPeer: number;
  /** Last action kind per peer, to keep undo/redo unchained (see `generate`). */
  readonly #lastKind = new Map<number, Action["kind"]>();
  /**
   * Value after each peer's last undo/redo. Rust skips an undo item that turns
   * out to be a no-op and undoes the previous one too, which then chains; so
   * undo again only after the value changed.
   */
  readonly #valueAtUndo = new Map<number, string>();

  constructor(
    readonly engines: Record<EngineName, EngineModule>,
    readonly profile: FuzzProfile,
    readonly seed: number,
  ) {
    this.rng = new Rng(seed);
    this.#nextPeer = profile.peers + 1;
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
        // A peer can leave the run during these syncs (see applySync).
        if (from === to || !this.twins.has(from) || !this.twins.has(to)) continue;
        yield { kind: "sync", from, to, mode: "update", cross: this.profile.crossEngine };
      }
    }
  }

  generate(): Action {
    return this.generateAction();
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
    const canUndo =
      profile.undo && last !== "undo" && last !== "redo" && this.#changedSinceUndo(peer);
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
        const joined = this.#nextPeer++;
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
      case "move": {
        // A move onto the same index is a no-op; it would hide a chained undo.
        const from = rng.int(length);
        const to = (from + 1 + rng.int(length - 1)) % length;
        return { kind, peer, target, from, to };
      }
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
        const last = this.#lastKind.get(action.peer);
        if (
          action.kind === "undo"
            ? last === "undo" || last === "redo" || !this.#changedSinceUndo(action.peer)
            : last !== "undo"
        ) {
          throw new ChainedUndo(`${action.kind} after ${last ?? "nothing"}`);
        }
        this.#lastKind.set(action.peer, action.kind);
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
        this.#valueAtUndo.set(action.peer, stringify(plain(twin.docs.rust.toJSON())));
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
        if (this.#revertIsUnspecified(twin, frontiers)) {
          this.#revertThroughRust(twin, frontiers);
        } else {
          twin.both("revertTo", (doc) => {
            doc.revertTo(frontiers);
            doc.commit();
          });
        }
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
            // A child created in the range whose map keys were all set and
            // deleted again: Rust lists them as deleted, loro.js (List, Map and
            // MovableList parents alike, on `main` too) omits the child. Rust
            // can also list a sequence whose delta is empty (two concurrent
            // moves of one element); loro.js omits it.
            .filter(([, diff]) => !onlyDeletedMapKeys(diff) && !emptyDelta(diff))
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

  /**
   * Whether the ops of a revert are not determined by the value it restores.
   * Rust applies a revert's container diffs in `FxHashMap` order within one
   * depth, so when several containers at the same depth change, the two
   * engines write the same values in a different op order. A revert's list ops
   * also follow the shape of `diff(current, target)`: an insert before or after
   * a deletion at the same index lands on either side of the deleted positions.
   * Both shapes describe the same change (see `eventShapeDifferences`). Twins
   * must share one history, so such a revert is checked by value and then
   * replicated.
   */
  #revertIsUnspecified(twin: Twin, frontiers: FrontiersLike): boolean {
    let rustDiff: [string, unknown][];
    let jsDiff: [string, unknown][];
    try {
      twin.both("commit before diff", (doc) => doc.commit());
      rustDiff = twin.docs.rust.diff(twin.docs.rust.frontiers(), frontiers, false);
      jsDiff = twin.docs.js.diff(twin.docs.js.frontiers(), frontiers, false);
    } catch {
      return false;
    }
    // Depths at the target: a container the revert brings back has no path now.
    // Rust cannot fork a shallow document; without the target, such a
    // container's depth is unknown and the order counts as unspecified.
    let atTarget: DocLike | undefined;
    try {
      atTarget = twin.docs.rust.forkAt(frontiers);
    } catch {
      atTarget = undefined;
    }
    const depths = new Map<number, number>();
    for (const [id] of rustDiff) {
      const path = (atTarget ?? twin.docs.rust).getPathToContainer(id);
      if (path === undefined && atTarget === undefined) return true;
      const depth = path?.length ?? 0;
      depths.set(depth, (depths.get(depth) ?? 0) + 1);
    }
    if ([...depths.values()].some((count) => count > 1)) return true;
    const shape = (diff: [string, unknown][]) =>
      new Map(diff.map(([id, change]) => [id, orderedDiff(change)]));
    const rustShape = shape(rustDiff);
    if (
      !isDeepStrictEqual(rustShape, shape(jsDiff.filter(([id]) => rustShape.has(id))))
    ) {
      eventShapeDifferences.count += 1;
      return true;
    }
    return false;
  }

  #revertThroughRust(twin: Twin, frontiers: FrontiersLike): void {
    twin.both("commit before revert", (doc) => doc.commit());
    assertTwinAgrees(twin, "after commit before revert", {
      metadata: this.profile.metadata,
      events: this.profile.events,
    });
    const fork = twin.docs.js.fork();
    fork.revertTo(frontiers);
    fork.commit();
    const before = vvToMap(twin.docs.js.oplogVersion());
    twin.docs.rust.revertTo(frontiers);
    twin.docs.rust.commit();
    const expected = plain(twin.docs.rust.toJSON());
    if (!isDeepStrictEqual(plain(fork.toJSON()), expected)) {
      throw new Divergence(
        `${twin.name}: revertTo(${stringify(frontiers)}) values differ\n` +
          `  rust: ${stringify(expected)}\n  js:   ${stringify(plain(fork.toJSON()))}`,
      );
    }
    twin.docs.js.import(
      twin.docs.rust.export({
        mode: "update",
        from: new this.engines.rust.VersionVector(before),
      }),
    );
    // The Rust half reports a local change, the loro.js half an import.
    twin.events.rust.length = 0;
    twin.events.js.length = 0;
  }

  #changedSinceUndo(peer: number): boolean {
    const previous = this.#valueAtUndo.get(peer);
    return (
      previous === undefined ||
      previous !== stringify(plain(this.twin(peer).docs.rust.toJSON()))
    );
  }

  twin(peer: number): Twin {
    return this.twins.get(peer)!;
  }

  historyAt(twin: Twin, index: number): FrontiersLike {
    return index < 0 ? [] : (twin.history[index] ?? []);
  }

  applyEdit(action: Extract<Action, { target: string }>): void {
    const twin = this.twin(action.peer);
    const applied = twin.both(action.kind, (doc, engine) => {
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
    // Only a real edit breaks an undo chain (see `generateAction`).
    if (applied !== undefined && (action.kind !== "move" || action.from !== action.to)) {
      this.#lastKind.set(action.peer, action.kind);
    }
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
    let imported: unknown;
    try {
      imported = target.both(
        `import from ${source.name} (${action.mode})`,
        (doc, engine) =>
          action.mode === "batch"
            ? doc.importBatch(blobs[engine])
            : doc.import(blobs[engine][0]!),
      );
    } finally {
      target.batchImport = false;
    }
    if (imported === undefined) {
      // Both rejected it, typically a change concurrent with a shallow root.
      // Rust keeps the blob's other changes; loro.js imports atomically. The
      // halves can now differ by design, so this peer leaves the run.
      this.twins.delete(action.to);
      return;
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
        // Rust exports nothing for a span that starts in history trimmed by a
        // shallow snapshot (context/loro-js-movable-list.md), so spans start at
        // the shallow root.
        const trimmed = vvToMap(doc.shallowSinceVV());
        const spans = [...vvToMap(doc.oplogVersion())].flatMap(([peer, end]) => {
          const start = Math.max(receiverVersion.get(peer) ?? 0, trimmed.get(peer) ?? 0);
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
    this.#nextPeer = Math.max(this.#nextPeer, action.peer + 1);
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
      bytes[engine] = source.docs[sourceEngine].export({ mode: "snapshot" });
    }
    if (action.shallow) {
      // loro.js roots a shallow snapshot at the meet of the requested heads,
      // Rust at the latest single-head critical version (loro-dev/loro#1095),
      // so replicas made from each keep different history. Both halves load
      // Rust's; loro.js's must still import into both engines with its value.
      const rustShallow = source.docs.rust.export({
        mode: "shallow-snapshot",
        frontiers,
      });
      const jsShallow = source.docs.js.export({ mode: "shallow-snapshot", frontiers });
      const expected = plain(source.docs.rust.toJSON());
      for (const engine of ENGINES) {
        const check = new this.engines[engine].LoroDoc();
        check.import(jsShallow);
        if (!isDeepStrictEqual(plain(check.toJSON()), expected)) {
          throw new Divergence(
            `${engine} imported loro.js's shallow snapshot of ${source.name} as ` +
              `${stringify(plain(check.toJSON()))}, expected ${stringify(expected)}`,
          );
        }
      }
      bytes.rust = rustShallow;
      bytes.js = rustShallow;
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

/**
 * `canonicalDiff`, except that list deltas keep the order of inserts and
 * deletes at one index; adjacent items of one kind are still merged.
 */
function orderedDiff(diff: unknown): unknown {
  const typed = diff as { type: string; diff?: Record<string, unknown>[] };
  if (typed.type !== "list") return canonicalDiff(diff);
  const output: Record<string, unknown>[] = [];
  for (const item of typed.diff!) {
    const last = output.at(-1);
    if (last !== undefined && "insert" in item && "insert" in last) {
      last.insert = [...(last.insert as unknown[]), ...(plain(item.insert) as unknown[])];
    } else if (last !== undefined && "delete" in item && "delete" in last) {
      last.delete = (last.delete as number) + (item.delete as number);
    } else if (last !== undefined && "retain" in item && "retain" in last) {
      last.retain = (last.retain as number) + (item.retain as number);
    } else {
      output.push("insert" in item ? { insert: plain(item.insert) } : { ...item });
    }
  }
  return { type: "list", diff: output };
}

function emptyDelta(diff: unknown): boolean {
  const typed = diff as { type: string; diff?: unknown };
  return Array.isArray(typed.diff) && typed.diff.length === 0;
}

function onlyDeletedMapKeys(diff: unknown): boolean {
  const typed = diff as { type: string; updated?: Record<string, unknown> };
  return (
    typed.type === "map" &&
    Object.values(typed.updated ?? {}).every((value) => value === undefined)
  );
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

/**
 * A replayed or minimized trace that chains undo, which the generator never
 * does (see `generateAction`); `replayTrace` reports it as a crash.
 */
class ChainedUndo extends Error {}

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
