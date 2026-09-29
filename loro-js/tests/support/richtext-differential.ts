import * as Js from "../../src/index";
import type { Delta, Frontiers, LoroEventBatch, Value } from "../../src/index";
import type { RustReference } from "./rust-reference";

/**
 * Differential rich-text scenarios between loro.js and the Rust implementation
 * (`loro-crdt`). Every peer exists once per runtime; each action runs the same
 * public API call on both copies and then compares text, delta, attributes,
 * event-reconstructed state, and cursors. Syncs randomly move binary or JSON
 * updates within a runtime or across runtimes, so positions written by one
 * implementation are interpreted by the other.
 *
 * Actions are plain data with unresolved positions, so a failing sequence can be
 * replayed, shrunk (`shrinkActions`), and pasted into a regression test.
 */

type Runtime = "rust" | "js";
type Doc = Js.LoroDoc;
type Text = Js.LoroText;
type UndoManager = Js.UndoManager;
type Cursor = Js.Cursor;

export const STYLE_CONFIG = {
  bold: { expand: "after" },
  link: { expand: "none" },
  em: { expand: "before" },
  hl: { expand: "both" },
} as const;
const STYLE_KEYS = Object.keys(STYLE_CONFIG);
const STYLE_VALUES: Value[] = [true, false, 1, 2, "x", "y", { a: 1 }];
const TEXTS = [
  "a",
  "b",
  "cd",
  "xyz",
  "é",
  "中文",
  "😀",
  "a😀b",
  "👍🏽",
  "\n",
  "ab\ncd",
  "𝒳y",
];

export type Action =
  | {
      readonly type: "insert";
      readonly peer: number;
      readonly pos: number;
      readonly text: string;
      readonly utf8?: boolean;
    }
  | {
      readonly type: "delete";
      readonly peer: number;
      readonly pos: number;
      readonly len: number;
      readonly utf8?: boolean;
    }
  | {
      readonly type: "mark";
      readonly peer: number;
      readonly pos: number;
      readonly len: number;
      readonly key: string;
      readonly value: Value;
    }
  | {
      readonly type: "unmark";
      readonly peer: number;
      readonly pos: number;
      readonly len: number;
      readonly key: string;
    }
  | { readonly type: "commit"; readonly peer: number }
  | {
      readonly type: "list";
      readonly peer: number;
      readonly op: number;
      readonly pos: number;
      readonly to: number;
    }
  | {
      readonly type: "sync";
      readonly from: number;
      readonly to: number;
      readonly json: boolean;
      readonly cross: boolean;
    }
  | { readonly type: "checkout"; readonly peer: number; readonly version: number }
  | { readonly type: "undo" | "redo"; readonly peer: number }
  | {
      readonly type: "revert";
      readonly peer: number;
      readonly version: number;
      readonly inRust: boolean;
    }
  | {
      readonly type: "snapshot";
      readonly peer: number;
      readonly fromRust: boolean;
      readonly version: number;
    }
  | {
      readonly type: "shallow";
      readonly peer: number;
      readonly fromRust: boolean;
      readonly version: number;
    }
  | {
      readonly type: "cursor";
      readonly peer: number;
      readonly pos: number;
      readonly side: -1 | 0 | 1;
    };

export interface ScenarioOptions {
  readonly peers?: number;
  readonly undo?: boolean;
  readonly crossRuntimeSync?: boolean;
  readonly snapshots?: boolean;
  readonly snapshotCheckout?: boolean;
  /** Check out a shallow snapshot's retained versions (loro.js roots differ). */
  readonly shallowCheckout?: boolean;
  readonly checkout?: boolean;
  /**
   * Edit a movable list next to the text. Its set, move, and delete ops keep
   * checkouts off the incremental path, so they replay the text's records.
   */
  readonly movableList?: boolean;
  readonly revert?: boolean;
  readonly cursors?: boolean;
  readonly events?: boolean;
  readonly marks?: boolean;
  readonly compareOps?: boolean;
  /**
   * Run undo/redo in both runtimes and compare, instead of running it in
   * loro.js and importing the result into Rust.
   */
  readonly undoParity?: boolean;
}

const DEFAULT_OPTIONS: Required<ScenarioOptions> = {
  peers: 3,
  undo: true,
  crossRuntimeSync: true,
  snapshots: true,
  snapshotCheckout: true,
  shallowCheckout: false,
  checkout: true,
  movableList: true,
  revert: true,
  cursors: true,
  events: true,
  marks: true,
  compareOps: true,
  undoParity: false,
};

export class DifferentialFailure extends Error {}

class Random {
  #state: number;

  constructor(seed: number) {
    this.#state = (seed * 2_654_435_761 + 0x9e37_79b9) >>> 0 || 1;
  }

  next(): number {
    let value = this.#state;
    value ^= value << 13;
    value ^= value >>> 17;
    value ^= value << 5;
    this.#state = value >>> 0;
    return this.#state;
  }

  int(bound: number): number {
    return bound <= 0 ? 0 : this.next() % bound;
  }

  pick<T>(values: readonly T[]): T {
    return values[this.int(values.length)]!;
  }
}

/** Generates a replayable action list. Positions are resolved at run time. */
export function generateActions(
  seed: number,
  steps: number,
  options: ScenarioOptions = {},
): Action[] {
  const config = { ...DEFAULT_OPTIONS, ...options };
  const random = new Random(seed);
  const actions: Action[] = [];
  const peerIds = Array.from({ length: config.peers }, (_, index) => index + 1);
  const commitMaybe = (peer: number): void => {
    if (random.int(5) !== 0) actions.push({ type: "commit", peer });
  };
  for (let step = 0; step < steps; step += 1) {
    const peer = random.pick(peerIds);
    const roll = random.int(100);
    const pos = random.next();
    const len = 1 + random.int(6);
    if (roll < 32) {
      const text = random.int(4) === 0 ? `${step % 10}` : random.pick(TEXTS);
      actions.push({ type: "insert", peer, pos, text, utf8: roll >= 26 });
      commitMaybe(peer);
    } else if (roll < 48) {
      actions.push({ type: "delete", peer, pos, len, utf8: roll >= 44 });
      commitMaybe(peer);
    } else if (roll < 62 && config.marks) {
      const key = random.pick(STYLE_KEYS);
      const value = random.int(8) === 0 ? null : random.pick(STYLE_VALUES);
      actions.push({ type: "mark", peer, pos, len, key, value });
      commitMaybe(peer);
    } else if (roll < 68 && config.marks) {
      actions.push({ type: "unmark", peer, pos, len, key: random.pick(STYLE_KEYS) });
      commitMaybe(peer);
    } else if (roll < 76 || (roll < 80 && !config.movableList)) {
      const to = random.pick(peerIds.filter((id) => id !== peer));
      actions.push({
        type: "sync",
        from: peer,
        to,
        json: random.int(2) === 0,
        cross: config.crossRuntimeSync && random.int(2) === 0,
      });
    } else if (roll < 80) {
      actions.push({ type: "list", peer, op: random.int(4), pos, to: random.next() });
      commitMaybe(peer);
    } else if (roll < 84 && config.checkout) {
      actions.push({ type: "checkout", peer, version: random.next() });
    } else if (roll < 88 && config.undo) {
      actions.push({ type: random.int(3) === 0 ? "redo" : "undo", peer });
    } else if (roll < 90 && config.revert) {
      actions.push({
        type: "revert",
        peer,
        version: random.next(),
        inRust: random.int(2) === 0,
      });
    } else if (roll < 93 && config.snapshots) {
      actions.push({
        type: "snapshot",
        peer,
        fromRust: random.int(2) === 0,
        version: random.next(),
      });
    } else if (roll < 95 && config.snapshots) {
      actions.push({
        type: "shallow",
        peer,
        fromRust: random.int(2) === 0,
        version: random.next(),
      });
    } else if (roll < 98 && config.cursors) {
      actions.push({ type: "cursor", peer, pos, side: random.pick([-1, 0, 1] as const) });
    } else {
      actions.push({ type: "commit", peer });
    }
  }
  return actions;
}

interface Replica {
  readonly runtime: Runtime;
  readonly doc: Doc;
  readonly shadow: EventShadow;
  readonly undo: UndoManager | undefined;
}

interface EventShadow {
  delta: Delta<string>[];
}

interface Peer {
  readonly id: number;
  readonly rust: Replica;
  readonly js: Replica;
  readonly versions: Frontiers[];
  readonly cursors: { rust: Cursor; js: Cursor }[];
}

export class RichtextScenario {
  readonly #rust: typeof Js;
  readonly #options: Required<ScenarioOptions>;
  readonly #peers: Peer[] = [];
  readonly #log: string[] = [];

  constructor(rust: RustReference, options: ScenarioOptions = {}) {
    this.#rust = rust as unknown as typeof Js;
    this.#options = { ...DEFAULT_OPTIONS, ...options };
    for (let index = 0; index < this.#options.peers; index += 1) {
      const id = index + 1;
      this.#peers.push({
        id,
        rust: this.#replica("rust", id),
        js: this.#replica("js", id),
        versions: [],
        cursors: [],
      });
    }
  }

  /** The replicas of a peer, for diagnostics. */
  replicas(peer: number): { rust: Doc; js: Doc } {
    const selected = this.#peer(peer);
    return { rust: selected.rust.doc, js: selected.js.doc };
  }

  run(actions: readonly Action[]): void {
    for (const action of actions) this.#apply(action);
    this.#syncAll();
    this.#compareAll("final sync");
  }

  #replica(runtime: Runtime, peer: number): Replica {
    const module = runtime === "rust" ? this.#rust : Js;
    const doc = new module.LoroDoc();
    doc.setPeerId(peer);
    doc.configTextStyle(STYLE_CONFIG);
    doc.getText("t");
    const shadow: EventShadow = { delta: [] };
    // Only loro.js events are checked: they must reproduce loro.js state, which
    // is compared with Rust state. Rust itself panics ("Op/hint length
    // mismatch", `txn.rs`) when a subscribed transaction merges deletes whose
    // UTF-16 and Unicode lengths differ, even after unsubscribing.
    if (this.#options.events && runtime === "js") {
      doc.subscribe((batch: LoroEventBatch) => {
        for (const event of batch.events) {
          if (event.diff.type !== "text") continue;
          shadow.delta = applyDelta(shadow.delta, event.diff.diff as Delta<string>[]);
        }
      });
    }
    const undo =
      this.#options.undo && (runtime === "js" || this.#options.undoParity)
        ? new module.UndoManager(doc, { mergeInterval: 0, maxUndoSteps: 100 })
        : undefined;
    return { runtime, doc, shadow, undo };
  }

  #peer(id: number): Peer {
    return this.#peers[(id - 1) % this.#peers.length]!;
  }

  #apply(action: Action): void {
    switch (action.type) {
      case "insert": {
        const peer = this.#peer(action.peer);
        const position = pickBoundary(this.#currentText(peer), action.pos, action.utf8);
        const text = action.text;
        if (action.utf8) {
          this.#both(peer, `insertUtf8(${position}, ${JSON.stringify(text)})`, (r) =>
            this.#text(r).insertUtf8(position, text),
          );
        } else {
          this.#both(peer, `insert(${position}, ${JSON.stringify(text)})`, (r) =>
            this.#text(r).insert(position, text),
          );
        }
        return;
      }
      case "delete": {
        const peer = this.#peer(action.peer);
        const range = pickRange(
          this.#currentText(peer),
          action.pos,
          action.len,
          action.utf8,
        );
        if (range === undefined) return;
        const [start, end] = range;
        if (action.utf8) {
          this.#both(peer, `deleteUtf8(${start}, ${end - start})`, (r) =>
            this.#text(r).deleteUtf8(start, end - start),
          );
        } else {
          this.#both(peer, `delete(${start}, ${end - start})`, (r) =>
            this.#text(r).delete(start, end - start),
          );
        }
        return;
      }
      case "mark": {
        const peer = this.#peer(action.peer);
        const range = pickRange(this.#currentText(peer), action.pos, action.len);
        if (range === undefined) return;
        const [start, end] = range;
        const { key, value } = action;
        this.#both(
          peer,
          `mark(${start}..${end}, ${key}, ${JSON.stringify(value)})`,
          (r) => this.#text(r).mark({ start, end }, key, value),
        );
        return;
      }
      case "unmark": {
        const peer = this.#peer(action.peer);
        const range = pickRange(this.#currentText(peer), action.pos, action.len);
        if (range === undefined) return;
        const [start, end] = range;
        this.#both(peer, `unmark(${start}..${end}, ${action.key})`, (r) =>
          this.#text(r).unmark({ start, end }, action.key),
        );
        return;
      }
      case "commit":
        this.#commit(this.#peer(action.peer));
        return;
      case "list":
        this.#editList(action);
        return;
      case "sync":
        this.#sync(action);
        return;
      case "checkout": {
        const peer = this.#peer(action.peer);
        this.#commit(peer);
        // Two versions in a row, so transitions also run between historical
        // versions, not only from and back to the latest one.
        for (const roll of [action.version, secondRoll(action.version)]) {
          const version = this.#versionToVisit(peer, roll);
          if (version === undefined) continue;
          this.#both(peer, `checkout(${JSON.stringify(version)})`, (r) =>
            r.doc.checkout(version),
          );
        }
        this.#both(peer, "checkoutToLatest", (r) => r.doc.checkoutToLatest());
        return;
      }
      case "undo":
      case "redo": {
        // loro.js's undo is a separate implementation that does not produce
        // Rust's ops (it does not transform against remote edits, for example).
        // Run it in loro.js and check that Rust reads the resulting ops the same.
        const peer = this.#peer(action.peer);
        this.#commit(peer);
        if (this.#options.undoParity) {
          this.#both(peer, action.type, (replica) => {
            if (action.type === "redo") replica.undo?.redo();
            else replica.undo?.undo();
          });
          this.#commit(peer);
          return;
        }
        this.#oneSided(peer, peer.js, action.type, () => {
          if (action.type === "redo") peer.js.undo?.redo();
          else peer.js.undo?.undo();
        });
        return;
      }
      case "revert": {
        const peer = this.#peer(action.peer);
        if (peer.versions.length === 0) return;
        this.#commit(peer);
        const version = peer.versions[action.version % peer.versions.length]!;
        const label = `revertTo(${JSON.stringify(version)})`;
        // The resulting state must match, but the ops need not: Rust applies its
        // diff delta in document order, including where inserted text sits
        // relative to deleted text, while loro.js composes its diff first.
        const rustFork = peer.rust.doc.fork();
        const jsFork = peer.js.doc.fork();
        rustFork.revertTo(version);
        jsFork.revertTo(version);
        this.#expectDelta(
          jsFork,
          normalizeDelta(rustFork.getText("t").toDelta()),
          `${label} on forks`,
        );
        const actor = action.inRust ? peer.rust : peer.js;
        this.#oneSided(peer, actor, label, () => actor.doc.revertTo(version));
        return;
      }
      case "snapshot":
        this.#snapshotRoundTrip(action);
        return;
      case "shallow":
        this.#shallowRoundTrip(action);
        return;
      case "cursor":
        this.#createCursor(action);
        return;
    }
  }

  /**
   * A recorded commit version, or for odd rolls the version right after a
   * random op in the history. The latter can split a change or an op, such as
   * a mark whose end anchor is not included yet.
   */
  #versionToVisit(peer: Peer, roll: number): Frontiers | undefined {
    if (roll % 2 === 1) {
      const counters = [...peer.rust.doc.oplogVersion().toJSON()].filter(
        ([, end]) => end > 0,
      );
      if (counters.length > 0) {
        const [id, end] = counters[(roll >>> 1) % counters.length]!;
        return [{ peer: id, counter: (roll >>> 5) % end }];
      }
    }
    return peer.versions.length === 0
      ? undefined
      : peer.versions[roll % peer.versions.length];
  }

  #text(replica: Replica): Text {
    return replica.doc.getText("t");
  }

  #currentText(peer: Peer): string {
    return this.#text(peer.rust).toString();
  }

  #both(peer: Peer, label: string, action: (replica: Replica) => void): void {
    this.#log.push(`peer ${peer.id}: ${label}`);
    const errors: { runtime: Runtime; error: unknown }[] = [];
    for (const replica of [peer.rust, peer.js]) {
      try {
        action(replica);
      } catch (error) {
        errors.push({ runtime: replica.runtime, error });
      }
    }
    if (errors.length === 1) {
      this.#fail(
        `${label}: only ${errors[0]!.runtime} threw: ${String(errors[0]!.error)}`,
      );
    }
    this.#compare(peer, label);
  }

  /**
   * Runs an edit on one replica only, then imports its change into the other
   * replica of the same peer, so both keep identical histories.
   */
  #oneSided(peer: Peer, actor: Replica, label: string, edit: () => void): void {
    this.#log.push(`peer ${peer.id}: ${label} in ${actor.runtime}`);
    try {
      edit();
      actor.doc.commit();
    } catch (error) {
      this.#fail(`${label} in ${actor.runtime} threw: ${String(error)}`);
    }
    const other = actor === peer.rust ? peer.js : peer.rust;
    other.doc.import(actor.doc.export({ mode: "update" }));
    this.#compare(peer, `${label} in ${actor.runtime}`);
    peer.versions.push(peer.rust.doc.frontiers());
  }

  #commit(peer: Peer): void {
    this.#both(peer, "commit", (replica) => replica.doc.commit());
    const version = peer.rust.doc.frontiers();
    const previous = peer.versions.at(-1);
    if (previous === undefined || JSON.stringify(previous) !== JSON.stringify(version)) {
      peer.versions.push(version);
    }
  }

  #sync(action: Extract<Action, { type: "sync" }>): void {
    const from = this.#peer(action.from);
    const to = this.#peer(action.to);
    if (from === to) return;
    this.#commit(from);
    this.#commit(to);
    const cross = action.cross && this.#options.crossRuntimeSync;
    this.#log.push(
      `sync ${from.id} -> ${to.id} (${action.json ? "json" : "binary"}${cross ? ", cross" : ""})`,
    );
    const move = (source: Replica, target: Replica): void => {
      if (action.json) {
        target.doc.importJsonUpdates(JSON.stringify(source.doc.exportJsonUpdates()));
      } else {
        target.doc.import(source.doc.export({ mode: "update" }));
      }
    };
    move(cross ? from.js : from.rust, to.rust);
    move(cross ? from.rust : from.js, to.js);
    this.#compare(to, `after sync ${from.id} -> ${to.id}`);
    to.versions.push(to.rust.doc.frontiers());
  }

  #syncAll(): void {
    this.#log.push("sync all");
    for (const from of this.#peers) {
      for (const to of this.#peers) {
        if (from === to) continue;
        this.#commit(from);
        const cross = this.#options.crossRuntimeSync;
        to.rust.doc.import((cross ? from.js : from.rust).doc.export({ mode: "update" }));
        to.js.doc.import((cross ? from.rust : from.js).doc.export({ mode: "update" }));
      }
    }
  }

  /**
   * Edits the movable list in Rust and imports the change into loro.js. The
   * list itself is not compared, and it is never moved or deleted from: loro.js
   * and Rust disagree on such lists (loro-dev/loro#1132), and Rust panics when
   * it checks out a snapshot that loro.js wrote with the differing state.
   */
  #editList(action: Extract<Action, { type: "list" }>): void {
    const peer = this.#peer(action.peer);
    const list = peer.rust.doc.getMovableList("ml");
    const length = list.length;
    const insert = length === 0 || action.op === 0;
    this.#oneSided(peer, peer.rust, insert ? "list insert" : "list set", () => {
      if (insert) list.insert(action.pos % (length + 1), action.to % 100);
      else list.set(action.pos % length, action.to % 100);
    });
  }

  #snapshotRoundTrip(action: Extract<Action, { type: "snapshot" }>): void {
    const peer = this.#peer(action.peer);
    this.#commit(peer);
    const source = action.fromRust ? peer.rust : peer.js;
    const bytes = source.doc.export({ mode: "snapshot" });
    this.#log.push(`peer ${peer.id}: snapshot from ${source.runtime}`);
    // Versions in a row, so the state holds elements hidden by earlier
    // checkouts when a later checkout, diff, or attach moves forward again.
    const rolls = [action.version, secondRoll(action.version)];
    rolls.push(secondRoll(rolls[1]!));
    const versions = rolls
      .map((roll) => this.#versionToVisit(peer, roll))
      .filter((version) => version !== undefined);
    this.#compareImported(peer, bytes, `snapshot from ${source.runtime}`, versions);
  }

  #shallowRoundTrip(action: Extract<Action, { type: "shallow" }>): void {
    const peer = this.#peer(action.peer);
    if (peer.versions.length === 0) return;
    this.#commit(peer);
    const source = action.fromRust ? peer.rust : peer.js;
    const index = action.version % peer.versions.length;
    const frontiers = peer.versions[index]!;
    const bytes = source.doc.export({ mode: "shallow-snapshot", frontiers });
    this.#log.push(
      `peer ${peer.id}: shallow snapshot from ${source.runtime} at ${JSON.stringify(frontiers)}`,
    );
    const later = peer.versions.slice(index);
    this.#compareImported(
      peer,
      bytes,
      `shallow snapshot from ${source.runtime}`,
      this.#options.shallowCheckout ? [later[action.version % later.length]!] : [],
    );
  }

  /**
   * Imports `bytes` into fresh loro.js and Rust documents. loro.js must match the
   * Rust import of the same bytes; both must match the source unless Rust
   * disagrees with itself (a Rust shallow document can apply a delete whose
   * recorded IDs and positions disagree differently from a full one).
   */
  #compareImported(
    peer: Peer,
    bytes: Uint8Array,
    label: string,
    versions: readonly Frontiers[],
  ): void {
    const js = new Js.LoroDoc();
    js.import(bytes);
    const rust = new this.#rust.LoroDoc();
    rust.import(bytes);
    const reference = normalizeDelta(rust.getText("t").toDelta());
    const source = normalizeDelta(this.#text(peer.rust).toDelta());
    if (JSON.stringify(reference) !== JSON.stringify(source)) {
      // Rust's shallow import falls back to the recorded delete IDs when they
      // disagree with the positions, which diverges from its own full-history
      // result. loro.js keeps the full-history (position) result.
      this.#log.push("  (the Rust import differs from its source)");
    }
    this.#expectDelta(js, source, `${label}: loro.js import vs source`);
    if (!this.#options.snapshotCheckout || versions.length === 0) return;
    const both = (step: string, run: (doc: Doc) => void): boolean => {
      const errors: string[] = [];
      for (const doc of [rust, js]) {
        try {
          run(doc);
        } catch (error) {
          errors.push(String(error));
        }
      }
      if (errors.length === 1) {
        this.#fail(`${label}: ${step} threw only in one runtime: ${errors[0]}`);
      }
      return errors.length === 0;
    };
    const states: Delta<string>[][] = [];
    for (const version of versions) {
      const step = `checkout ${JSON.stringify(version)}`;
      if (!both(step, (doc) => doc.checkout(version))) return;
      states.push(normalizeDelta(rust.getText("t").toDelta()));
      this.#expectDelta(js, states.at(-1)!, `${label}: ${step}`);
    }
    states.splice(0, states.length - 2);
    if (versions.length >= 2) {
      const [from, to] = versions.slice(-2) as [Frontiers, Frontiers];
      const step = `diff ${JSON.stringify(from)} -> ${JSON.stringify(to)}`;
      const diffs: { rust?: unknown; js?: unknown } = {};
      if (
        both(step, (doc) => {
          const text = doc.diff(from, to).find(([id]) => id === "cid:root-t:Text")?.[1];
          diffs[doc === js ? "js" : "rust"] = text;
        })
      ) {
        // Applied to the state at `from`, both diffs must give the state at `to`.
        const applied = (diff: unknown): Delta<string>[] =>
          normalizeDelta(
            diff === undefined
              ? states[0]!
              : applyDelta(states[0]!, (diff as { diff: Delta<string>[] }).diff),
          );
        for (const [runtime, diff] of Object.entries(diffs)) {
          if (JSON.stringify(applied(diff)) !== JSON.stringify(states[1])) {
            this.#fail(`${label}: ${step}: the ${runtime} diff does not reach \`to\``);
          }
        }
      }
    }
    if (!both("attach", (doc) => doc.attach())) return;
    this.#expectDelta(js, source, `${label}: attach`);
  }

  #createCursor(action: Extract<Action, { type: "cursor" }>): void {
    const peer = this.#peer(action.peer);
    const position = pickBoundary(this.#currentText(peer), action.pos);
    const side = action.side;
    const rust = this.#text(peer.rust).getCursor(position, side);
    const js = this.#text(peer.js).getCursor(position, side);
    this.#log.push(`peer ${peer.id}: cursor(${position}, ${side})`);
    if ((rust === undefined) !== (js === undefined)) {
      this.#fail(`cursor(${position}, ${side}) existence differs`);
    }
    if (rust === undefined || js === undefined) return;
    if (
      JSON.stringify(rust.pos()) !== JSON.stringify(js.pos()) ||
      rust.side() !== js.side()
    ) {
      this.#fail(
        `cursor(${position}, ${side}) differs: rust ${JSON.stringify(rust.pos())}/${rust.side()} js ${JSON.stringify(js.pos())}/${js.side()}`,
      );
    }
    peer.cursors.push({ rust, js });
  }

  #compareCursors(peer: Peer, label: string): void {
    // Rust resolves a deleted target against the oplog head but converts the
    // position with the checked-out state, so detached results are not compared.
    if (peer.rust.doc.getPendingTxnLength() > 0 || peer.rust.doc.isDetached()) return;
    for (const cursor of peer.cursors) {
      const rust = peer.rust.doc.getCursorPos(cursor.rust);
      const js = peer.js.doc.getCursorPos(cursor.js);
      if (
        rust === undefined &&
        js !== undefined &&
        !recordedAsDeleted(peer.rust.doc, cursor.rust)
      ) {
        // Rust finds a deleted target through the recorded delete IDs, which its
        // WASM build can get wrong for astral text; loro.js resolves deletes by
        // position and still knows where the target was.
        continue;
      }
      if (rust?.offset !== js?.offset || rust?.side !== js?.side) {
        this.#fail(
          `${label}: cursor ${JSON.stringify(cursor.rust.pos())}/${cursor.rust.side()} position differs: rust ${JSON.stringify(rust && { offset: rust.offset, side: rust.side })} js ${JSON.stringify(js && { offset: js.offset, side: js.side })}`,
        );
      }
    }
  }

  #expectDelta(doc: Doc, expected: unknown, label: string): void {
    const actual = normalizeDelta(doc.getText("t").toDelta());
    if (JSON.stringify(actual) !== JSON.stringify(expected)) {
      this.#fail(
        `${label}: delta ${JSON.stringify(actual)} != expected ${JSON.stringify(expected)}`,
      );
    }
  }

  #compare(peer: Peer, label: string): void {
    const rustText = this.#text(peer.rust);
    const jsText = this.#text(peer.js);
    if (jsText.toDelta().some((item) => "insert" in item && item.insert.length === 0)) {
      this.#fail(`${label}: peer ${peer.id} loro.js delta has an empty insert`);
    }
    const rustDelta = normalizeDelta(rustText.toDelta());
    const jsDelta = normalizeDelta(jsText.toDelta());
    if (JSON.stringify(rustDelta) !== JSON.stringify(jsDelta)) {
      this.#fail(
        `${label}: peer ${peer.id} delta differs\n  rust ${JSON.stringify(rustDelta)}\n  js   ${JSON.stringify(jsDelta)}`,
      );
    }
    if (rustText.length !== jsText.length) {
      this.#fail(`${label}: length ${rustText.length} != ${jsText.length}`);
    }
    for (const unit of ["unicode", "utf8"] as const) {
      const rustLength = rustText.convertPos(rustText.length, "utf16", unit);
      const jsLength = jsText.convertPos(jsText.length, "utf16", unit);
      if (rustLength !== jsLength) {
        this.#fail(`${label}: ${unit} length ${rustLength} != ${jsLength}`);
      }
    }
    if (this.#options.events) {
      for (const replica of [peer.js]) {
        if (replica.doc.getPendingTxnLength() > 0) continue;
        const events = normalizeDelta(replica.shadow.delta);
        const state = normalizeDelta(this.#text(replica).toDelta());
        if (JSON.stringify(events) !== JSON.stringify(state)) {
          this.#fail(
            `${label}: ${replica.runtime} events do not reproduce the state\n  events ${JSON.stringify(events)}\n  state  ${JSON.stringify(state)}`,
          );
        }
      }
    }
    if (this.#options.cursors) this.#compareCursors(peer, label);
    if (this.#options.compareOps) this.#compareOps(peer, label);
  }

  #compareOps(peer: Peer, label: string): void {
    const rust = atomicOps(peer.rust.doc);
    const js = atomicOps(peer.js.doc);
    const index = rust.findIndex((atom, position) => atom !== js[position]);
    if (index >= 0 || rust.length !== js.length) {
      const at = index >= 0 ? index : Math.min(rust.length, js.length);
      this.#fail(
        `${label}: peer ${peer.id} ops differ at atom ${at}\n  rust ${rust[at]}\n  js   ${js[at]}`,
      );
    }
  }

  #compareAll(label: string): void {
    const expected = normalizeDelta(this.#text(this.#peers[0]!.rust).toDelta());
    for (const peer of this.#peers) {
      this.#compare(peer, label);
      this.#expectDelta(peer.js.doc, expected, `${label}: converge peer ${peer.id}`);
    }
  }

  #fail(message: string): never {
    throw new DifferentialFailure(
      `${message}\nsteps:\n  ${this.#log.slice(-40).join("\n  ")}`,
    );
  }
}

/**
 * The JSON history as atomic ops sorted by ID: one entry per inserted scalar
 * with its entity position, plus every other non-delete op. Rust merges adjacent
 * ops of merged changes depending on internal encoding state, so this form
 * ignores how atoms are grouped into ops.
 */
/**
 * A second roll derived from `roll`. The product's high bits, because the
 * multiplier is 1 modulo 16, so its low bits repeat `roll`'s.
 */
function secondRoll(roll: number): number {
  return Math.imul(roll, 0x9e37_79b1) >>> 16;
}

function atomicOps(doc: Doc): string[] {
  const json = doc.exportJsonUpdates();
  const peers = json.peers;
  const peerOf = (value: string): string =>
    peers == null ? value : peers[Number(value)]!;
  const expand = (value: string): [number, string] => {
    const [counter, peer] = value.split("@");
    return [Number(counter), peerOf(peer!)];
  };
  const atoms: { peer: string; counter: number; text: string }[] = [];
  for (const change of json.changes) {
    const [, peer] = expand(change.id);
    for (const op of change.ops) {
      if (op.container !== "cid:root-t:Text") continue;
      const content = op.content as Record<string, unknown>;
      const push = (offset: number, text: string): void => {
        atoms.push({ peer, counter: op.counter + offset, text });
      };
      if (content.type === "insert" && typeof content.text === "string") {
        // One atom per Unicode scalar, each with its entity position.
        let index = 0;
        for (const char of content.text) {
          push(index, `insert ${Number(content.pos) + index} ${JSON.stringify(char)}`);
          index += 1;
        }
      } else if (content.type === "delete") {
        // Deletes are compared through state: Rust's WASM build can record
        // delete IDs that disagree with the positions it applies (astral text).
        continue;
      } else {
        push(0, JSON.stringify(content));
      }
    }
  }
  return atoms
    .sort((left, right) =>
      left.peer < right.peer
        ? -1
        : left.peer > right.peer
          ? 1
          : left.counter - right.counter,
    )
    .map((atom) => `${atom.counter}@${atom.peer} ${atom.text}`);
}

/** Whether some recorded delete op names the cursor's target, as Rust requires. */
function recordedAsDeleted(doc: Doc, cursor: Cursor): boolean {
  const id = cursor.pos();
  if (id === undefined) return true;
  const json = doc.exportJsonUpdates();
  const peers = json.peers;
  for (const change of json.changes) {
    for (const op of change.ops) {
      const content = op.content as { type: string; len?: number; start_id?: string };
      if (content.type !== "delete" || content.start_id === undefined) continue;
      const [counter, peerIndex] = content.start_id.split("@");
      const peer = peers == null ? peerIndex : peers[Number(peerIndex)];
      const start = Number(counter);
      const length = Math.abs(content.len ?? 0);
      if (peer === id.peer && id.counter >= start && id.counter < start + length) {
        return true;
      }
    }
  }
  return false;
}

/** Runs one scenario and returns the failure message, if any. */
export function runActions(
  rust: RustReference,
  actions: readonly Action[],
  options: ScenarioOptions = {},
): string | undefined {
  try {
    new RichtextScenario(rust, options).run(actions);
    return undefined;
  } catch (error) {
    return error instanceof DifferentialFailure
      ? error.message
      : error instanceof Error
        ? (error.stack ?? error.message)
        : String(error);
  }
}

/**
 * Greedily removes actions while the scenario still fails the same way (same
 * first failure line after replacing numbers), so shrinking does not wander
 * into an unrelated divergence.
 */
export function shrinkActions(
  rust: RustReference,
  actions: readonly Action[],
  options: ScenarioOptions = {},
): Action[] {
  let current = [...actions];
  const original = runActions(rust, current, options);
  if (original === undefined) return current;
  const kind = failureKind(original);
  const stillFails = (candidate: readonly Action[]): boolean => {
    const failure = runActions(rust, candidate, options);
    return failure !== undefined && failureKind(failure) === kind;
  };
  for (let chunk = Math.max(1, current.length >> 1); chunk >= 1; chunk >>= 1) {
    let index = 0;
    while (index < current.length) {
      const candidate = [...current.slice(0, index), ...current.slice(index + chunk)];
      if (stillFails(candidate)) current = candidate;
      else index += chunk;
    }
  }
  return current;
}

function failureKind(failure: string): string {
  const line = failure.split("\n")[0]!;
  const colon = line.indexOf(": ");
  return (colon < 0 ? line : line.slice(colon + 2)).replace(/[0-9]+/g, "N").slice(0, 40);
}

function boundaries(value: string, utf8 = false): number[] {
  const output = [0];
  let offset = 0;
  for (const scalar of value) {
    offset += utf8 ? new TextEncoder().encode(scalar).length : scalar.length;
    output.push(offset);
  }
  return output;
}

function pickBoundary(value: string, raw: number, utf8 = false): number {
  const all = boundaries(value, utf8);
  return all[raw % all.length]!;
}

function pickRange(
  value: string,
  raw: number,
  length: number,
  utf8 = false,
): [number, number] | undefined {
  const all = boundaries(value, utf8);
  if (all.length < 2) return undefined;
  const start = raw % (all.length - 1);
  const end = Math.min(all.length - 1, start + length);
  return [all[start]!, all[end]!];
}

export function normalizeDelta(delta: readonly Delta<string>[]): Delta<string>[] {
  const output: { insert: string; attributes?: Record<string, Value> }[] = [];
  for (const item of delta) {
    if (!("insert" in item) || item.insert.length === 0) continue;
    const attributes = normalizeAttributes(item.attributes);
    const previous = output.at(-1);
    if (
      previous !== undefined &&
      JSON.stringify(previous.attributes) === JSON.stringify(attributes)
    ) {
      previous.insert += item.insert;
    } else {
      output.push(
        attributes === undefined
          ? { insert: item.insert }
          : { insert: item.insert, attributes },
      );
    }
  }
  return output;
}

function normalizeAttributes(
  attributes: Readonly<Record<string, Value>> | undefined,
): Record<string, Value> | undefined {
  if (attributes === undefined) return undefined;
  const entries = Object.entries(attributes)
    .filter(([, value]) => value !== null && value !== undefined)
    .sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0));
  return entries.length === 0 ? undefined : Object.fromEntries(entries);
}

/** Applies a Quill-style text delta; `null` attribute values remove the key. */
export function applyDelta(
  document: readonly Delta<string>[],
  change: readonly Delta<string>[],
): Delta<string>[] {
  const chars: { value: string; attributes: Record<string, Value> }[] = [];
  for (const item of document) {
    if (!("insert" in item)) continue;
    for (let offset = 0; offset < item.insert.length; offset += 1) {
      chars.push({ value: item.insert[offset]!, attributes: { ...item.attributes } });
    }
  }
  let index = 0;
  for (const item of change) {
    if ("insert" in item) {
      const attributes = normalizeAttributes(item.attributes) ?? {};
      for (let offset = 0; offset < item.insert.length; offset += 1) {
        chars.splice(index, 0, {
          value: item.insert[offset]!,
          attributes: { ...attributes },
        });
        index += 1;
      }
    } else if ("delete" in item) {
      chars.splice(index, item.delete);
    } else {
      for (let offset = 0; offset < item.retain; offset += 1) {
        const target = chars[index + offset];
        if (target === undefined) break;
        for (const [key, value] of Object.entries(item.attributes ?? {})) {
          if (value === null || value === undefined) delete target.attributes[key];
          else target.attributes[key] = value;
        }
      }
      index += item.retain;
    }
  }
  return normalizeDelta(
    chars.map((char) => ({ insert: char.value, attributes: char.attributes })),
  );
}
