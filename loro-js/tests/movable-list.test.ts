import { readFileSync } from "node:fs";

import { describe, expect, test } from "vitest";

import {
  decodeFastSnapshot,
  decodeSstable,
  decodeStateSnapshotStore,
  encodeStateSnapshotStore,
} from "../src/codec";
import {
  LoroDoc,
  LoroList,
  LoroMap,
  LoroMovableList,
  LoroText,
  UndoManager,
  type LoroEventBatch,
} from "../src/index";

/**
 * MovableList regressions against the Rust model. Expected values were taken
 * from the Rust implementation (loro-crdt); see context/loro-js-movable-list.md.
 */

const fixture = (name: string): Uint8Array =>
  new Uint8Array(readFileSync(new URL(`./fixtures/rust/${name}`, import.meta.url)));

function doc(peer: number): LoroDoc {
  const created = new LoroDoc();
  created.setPeerId(peer);
  return created;
}

function metadata(list: LoroMovableList): (string | undefined)[][] {
  return Array.from({ length: list.length }, (_, index) => [
    list.getCreatorAt(index),
    list.getLastMoverAt(index),
    list.getLastEditorAt(index),
  ]);
}

function sync(...docs: LoroDoc[]): void {
  for (const source of docs) {
    for (const target of docs) {
      if (source !== target) target.import(source.export({ mode: "update" }));
    }
  }
}

describe("MovableList positions and elements", () => {
  test("a concurrent move keeps an element another peer deleted", () => {
    const a = doc(1);
    const list = a.getMovableList("list");
    for (const value of ["a", "b", "c"]) list.push(value);
    a.commit();
    const b = doc(2);
    b.import(a.export({ mode: "update" }));

    list.delete(1, 1);
    a.commit();
    b.getMovableList("list").move(1, 2);
    b.getMovableList("list").set(2, "B");
    b.commit();
    sync(a, b);

    expect(a.toJSON()).toEqual({ list: ["a", "c", "B"] });
    expect(b.toJSON()).toEqual({ list: ["a", "c", "B"] });
    const c = doc(3);
    c.import(a.export({ mode: "update" }));
    expect(c.toJSON()).toEqual({ list: ["a", "c", "B"] });
  });

  test("a delete after a move removes the moved element", () => {
    const imported = new LoroDoc();
    imported.import(fixture("movable-move-delete.blob"));
    expect(imported.toJSON()).toEqual({ list: ["b"] });

    const local = doc(1);
    const list = local.getMovableList("list");
    list.push("a");
    list.push("b");
    local.commit();
    list.move(0, 1);
    local.commit();
    list.delete(1, 1);
    local.commit();
    const replica = new LoroDoc();
    replica.import(local.export({ mode: "update" }));
    expect(replica.toJSON()).toEqual({ list: ["b"] });
  });

  test("the later of two concurrent moves wins; the loser position stays dead", () => {
    const a = doc(1);
    const list = a.getMovableList("list");
    for (const value of ["a", "b", "c"]) list.push(value);
    a.commit();
    const b = doc(2);
    b.import(a.export({ mode: "update" }));
    list.move(0, 2);
    a.commit();
    b.getMovableList("list").move(0, 1);
    b.commit();
    sync(a, b);
    expect(a.toJSON()).toEqual(b.toJSON());
    expect(a.toJSON()).toEqual({ list: ["b", "a", "c"] });
    // Inserting at the end goes after the dead position of the losing move.
    list.push("d");
    a.commit();
    sync(a, b);
    expect(b.toJSON()).toEqual({ list: ["b", "a", "c", "d"] });
  });

  test("reports the creator, last mover and last editor", () => {
    const a = doc(1);
    const list = a.getMovableList("list");
    list.push("a");
    list.push("b");
    a.commit();
    const b = doc(2);
    b.import(a.export({ mode: "update" }));
    const other = b.getMovableList("list");
    other.set(0, "A");
    other.move(1, 0);
    b.commit();
    expect(b.toJSON()).toEqual({ list: ["b", "A"] });
    expect(metadata(other)).toEqual([
      ["1", "2", "1"],
      ["1", "1", "2"],
    ]);
    a.import(b.export({ mode: "update" }));
    expect(metadata(list)).toEqual(metadata(other));
  });

  test("concurrent branches converge in any import order", () => {
    let seed = 7;
    const next = (limit: number): number => {
      seed = (seed * 1_103_515_245 + 12_345) % 2_147_483_648;
      return seed % limit;
    };
    const base = doc(1);
    for (let index = 0; index < 20; index += 1) base.getMovableList("list").push(index);
    base.commit();
    const version = base.oplogVersion();
    const branches = [2, 3, 4].map((peer) => {
      const branch = base.fork();
      branch.setPeerId(peer);
      const list = branch.getMovableList("list");
      for (let step = 0; step < 60; step += 1) {
        const kind = next(4);
        if (kind === 0) list.insert(next(list.length + 1), `${peer}-${step}`);
        else if (kind === 1 && list.length > 1) list.delete(next(list.length), 1);
        else if (kind === 2) list.set(next(list.length), -step);
        else list.move(next(list.length), next(list.length));
        if (step % 7 === 6) branch.commit();
      }
      branch.commit();
      return branch.export({ mode: "update", from: version });
    });

    const results = [
      [0, 1, 2],
      [2, 1, 0],
      [1, 0, 2],
    ].map((order) => {
      const target = base.fork();
      for (const index of order) target.import(branches[index]!);
      return target;
    });
    const batched = base.fork();
    batched.importBatch(branches);
    const replayed = new LoroDoc();
    replayed.import(results[0]!.export({ mode: "update" }));
    for (const target of [...results, batched, replayed]) {
      expect(target.toJSON()).toEqual(results[0]!.toJSON());
      expect(metadata(target.getMovableList("list"))).toEqual(
        metadata(results[0]!.getMovableList("list")),
      );
    }
  });

  test("replacing a MovableList value with a plain object emits its event", () => {
    const d = doc(1);
    const events: LoroEventBatch[] = [];
    d.subscribe((event) => events.push(event));
    const map = d.getMap("m");
    map.setContainer("k", new LoroMovableList()).push(1);
    d.commit();
    map.set("k", { n: 2 });
    d.commit();
    expect(d.toJSON()).toEqual({ m: { k: { n: 2 } } });
    expect(events.at(-1)!.events.map(({ diff }) => diff)).toEqual([
      { type: "map", updated: { k: { n: 2 } } },
    ]);
  });

  test("records a set even when the value does not change", () => {
    const a = doc(1);
    const list = a.getMovableList("list");
    list.push("same");
    a.commit();
    const b = doc(2);
    b.import(a.export({ mode: "update" }));
    b.getMovableList("list").set(0, "same");
    b.commit();
    a.import(b.export({ mode: "update" }));
    expect(list.getLastEditorAt(0)).toBe("2");
  });
});

describe("MovableList snapshots use Rust's layout", () => {
  test("reads Rust's list item, element and last-set IDs", () => {
    const hydrated = new LoroDoc();
    hydrated.import(fixture("movable-snapshot.blob"));
    const list = hydrated.getMovableList("list");
    expect(hydrated.toJSON()).toEqual({ list: ["B2", "B", "D", "d"] });
    expect(metadata(list)).toEqual([
      ["1", "1", "1"],
      ["1", "1", "2"],
      ["1", "3", "3"],
      ["1", "1", "1"],
    ]);

    // Concurrent edits made on a Rust replica and on a Rust peer that loaded
    // the snapshot still converge (the latter replays history first).
    hydrated.import(fixture("movable-snapshot-peer4.blob"));
    hydrated.import(fixture("movable-snapshot-peer3.blob"));
    expect(hydrated.toJSON()).toEqual({ list: ["y", "d", "x", "B", "D"] });
    expect(metadata(list)).toEqual([
      ["3", "3", "3"],
      ["1", "4", "1"],
      ["4", "4", "4"],
      ["1", "1", "2"],
      ["1", "3", "3"],
    ]);
  });

  test("writes the MovableList state byte-for-byte like Rust", () => {
    const rustSnapshot = decodeFastSnapshot(fixture("movable-snapshot.blob"));
    const hydrated = new LoroDoc();
    hydrated.import(fixture("movable-snapshot-updates.blob"));
    const exported = decodeFastSnapshot(hydrated.export({ mode: "snapshot" }));
    const listState = (bytes: Uint8Array): Uint8Array => {
      const store = decodeStateSnapshotStore(bytes);
      if (store.kind !== "sstable") throw new Error("expected state entries");
      const entry = store.containers.find(
        ({ id }) => id.kind === "root" && id.name === "list",
      );
      return encodeStateSnapshotStore(
        { kind: "sstable", frontiers: undefined, containers: [entry!] },
        { compression: "none" },
      );
    };
    expect(listState(exported.state)).toEqual(listState(rustSnapshot.state));
    expect(decodeSstable(exported.oplog).length).toBeGreaterThan(0);
  });
});

describe("MovableList versions, undo and diffs", () => {
  test("checkout selects the winning position and value at each version", () => {
    const a = doc(1);
    const list = a.getMovableList("list");
    for (const value of ["a", "b", "c"]) list.push(value);
    a.commit();
    const start = a.frontiers();
    list.move(0, 2);
    list.set(0, "B");
    a.commit();
    const moved = a.frontiers();
    list.delete(2, 1);
    a.commit();
    a.checkout(start);
    expect(a.toJSON()).toEqual({ list: ["a", "b", "c"] });
    expect(metadata(list)).toEqual([
      ["1", "1", "1"],
      ["1", "1", "1"],
      ["1", "1", "1"],
    ]);
    a.checkout(moved);
    expect(a.toJSON()).toEqual({ list: ["B", "c", "a"] });
    a.checkoutToLatest();
    expect(a.toJSON()).toEqual({ list: ["B", "c"] });
    // Rust: [delete 1, insert B, delete 1, retain 1, insert a], the same delta.
    expect(a.diff(start, moved, false)).toEqual([
      [
        "cid:root-list:MovableList",
        {
          type: "list",
          diff: [{ delete: 2 }, { insert: ["B"] }, { retain: 1 }, { insert: ["a"] }],
        },
      ],
    ]);
  });

  test("diff reports a child created in the range by its ops", () => {
    const a = doc(1);
    const list = a.getMovableList("list");
    list.push(0);
    a.commit();
    const start = a.frontiers();
    const child = list.insertContainer(0, new LoroMap());
    child.set("a", 1);
    a.commit();
    child.set("k", 9);
    a.commit();
    child.delete("k");
    a.commit();
    // Rust lists `k`, which was set and deleted again, as deleted.
    expect(a.diff(start, a.frontiers(), false)).toEqual([
      ["cid:root-list:MovableList", { type: "list", diff: [{ insert: [child] }] }],
      [child.id, { type: "map", updated: { a: 1, k: undefined } }],
    ]);
  });

  test("applyDiff writes Rust's ops: deletes first, then inserts", () => {
    const d = doc(1);
    const list = d.getMovableList("l");
    for (const value of ["a", "b", "c"]) list.push(value);
    d.commit();
    const version = d.oplogVersion();
    d.applyDiff([
      [
        list.id,
        {
          type: "list",
          diff: [
            { insert: ["x"] },
            { retain: 1 },
            { delete: 1 },
            { retain: 1 },
            { insert: ["y"] },
          ],
        },
      ],
    ]);
    d.commit();
    expect(d.toJSON()).toEqual({ l: ["x", "a", "c", "y"] });
    // Rust's `apply_delta` since loro-dev/loro#1138; op IDs must match its.
    expect(d.exportJsonUpdates(version).changes[0]!.ops.map((op) => op.content)).toEqual([
      { type: "delete", pos: 1, len: 1, start_id: "1@0" },
      { type: "insert", pos: 0, value: ["x"] },
      { type: "insert", pos: 3, value: ["y"] },
    ]);
  });

  test("revertTo deletes before it inserts, like Rust's apply_delta", () => {
    const p3 = doc(3);
    p3.getMovableList("list").insert(0, "v2");
    p3.commit();
    const p1 = doc(1);
    p1.import(p3.export({ mode: "update" }));
    const withV2 = p1.frontiers();
    p1.getMovableList("list").delete(0, 1);
    p1.getMovableList("list").insert(0, "v6");
    p1.commit();
    p3.import(p1.export({ mode: "update" }));
    p1.revertTo(withV2);
    p1.commit();
    p3.getMovableList("list").insert(0, "x");
    p3.commit();
    p1.import(p3.export({ mode: "update" }));
    // Since loro-dev/loro#1138 Rust deletes unclaimed elements first, so the
    // restored "v2" lands after the deleted positions and "x" before it.
    expect(p1.toJSON()).toEqual({ list: ["v2", "x"] });
  });

  test("undo of a move or set reinserts the old value, like Rust", () => {
    const d = doc(1);
    const undo = new UndoManager(d, { mergeInterval: 0 });
    const list = d.getMovableList("list");
    for (const value of ["a", "b", "c"]) list.push(value);
    d.commit();
    list.move(0, 2);
    d.commit();
    list.set(1, "C");
    d.commit();
    expect(d.toJSON()).toEqual({ list: ["b", "C", "a"] });
    undo.undo();
    expect(d.toJSON()).toEqual({ list: ["b", "c", "a"] });
    undo.undo();
    expect(d.toJSON()).toEqual({ list: ["a", "b", "c"] });
    undo.redo();
    expect(d.toJSON()).toEqual({ list: ["b", "c", "a"] });
  });

  test("a shallow document keeps root-time winners when it retreats", () => {
    const d = doc(1);
    const list = d.getMovableList("list");
    list.push("a");
    list.push("b");
    d.commit();
    const other = doc(2);
    other.import(d.export({ mode: "update" }));
    other.getMovableList("list").set(0, "A");
    other.getMovableList("list").move(1, 0);
    other.commit();
    d.import(other.export({ mode: "update" }));
    list.push("c");
    d.commit();
    const root = d.frontiers();
    list.set(0, "b");
    list.move(0, 2);
    d.commit();

    const shallow = new LoroDoc();
    shallow.import(d.export({ mode: "shallow-snapshot", frontiers: root }));
    expect(shallow.toJSON()).toEqual({ list: ["A", "c", "b"] });
    shallow.checkout(root);
    expect(shallow.toJSON()).toEqual({ list: ["b", "A", "c"] });
    // The last editor of "b" is its creator. Rust's shallow root seeds the
    // position writer instead and reports "2"; the full history agrees with 1.
    expect(metadata(shallow.getMovableList("list"))).toEqual([
      ["1", "2", "1"],
      ["1", "1", "2"],
      ["1", "1", "1"],
    ]);
  });

  test("a child container revived by a concurrent move reports its content", () => {
    const a = doc(1);
    const list = a.getMovableList("list");
    list.push("a");
    const text = list.insertContainer(1, new LoroText());
    text.insert(0, "hi");
    a.commit();
    const b = doc(2);
    b.import(a.export({ mode: "update" }));
    list.delete(1, 1);
    a.commit();
    b.getMovableList("list").move(1, 0);
    b.commit();
    const events: LoroEventBatch[] = [];
    a.subscribe((event) => events.push(event));
    a.import(b.export({ mode: "update" }));
    expect(a.toJSON()).toEqual({ list: ["hi", "a"] });
    const textEvent = events
      .flatMap((batch) => batch.events)
      .find((event) => event.target === text.id);
    expect(textEvent?.diff).toEqual({ type: "text", diff: [{ insert: "hi" }] });
  });

  test("checkout and diff stay linear in the number of inserts", () => {
    /** Best of three checkout round trips plus a diff over n scattered inserts. */
    function transitionsMs(size: number): number {
      const a = doc(1);
      const list = a.getMovableList("list");
      list.push(-1);
      a.commit();
      const start = a.frontiers();
      for (let index = 0; index < size; index += 1) {
        list.insert((index * 7_919) % (list.length + 1), index);
      }
      a.commit();
      const latest = a.frontiers();
      let best = Infinity;
      for (let run = 0; run < 3; run += 1) {
        const started = performance.now();
        a.checkout(start);
        expect(a.getMovableList("list").length).toBe(1);
        a.checkoutToLatest();
        expect(a.diff(start, latest).length).toBe(1);
        best = Math.min(best, performance.now() - started);
      }
      expect(a.frontiers()).toEqual(latest);
      return best;
    }
    // Each insert op walked the whole position tree to check that the
    // transition could skip the replay (8x the inserts took about 90x the time).
    const ratio = transitionsMs(8_000) / transitionsMs(1_000);
    expect(ratio).toBeLessThan(24);
  }, 60_000);
});

describe("MovableList cursors", () => {
  test("anchor the list item, which a move leaves in place", () => {
    const d = doc(1);
    const list = d.getMovableList("list");
    for (const value of ["a", "b", "c"]) list.push(value);
    d.commit();
    const cursor = list.getCursor(1)!;
    expect(cursor.pos()).toEqual({ peer: "1", counter: 1 });
    expect(d.getCursorPos(cursor)?.offset).toBe(1);
    list.move(1, 2);
    d.commit();
    // "b"'s old list item was deleted by the move; the cursor now points at "c".
    expect(d.getCursorPos(cursor)?.offset).toBe(1);
    expect(list.toJSON()).toEqual(["a", "c", "b"]);
    const end = list.getCursor(3)!;
    expect(end.pos()).toBeUndefined();
    expect(d.getCursorPos(end)?.offset).toBe(3);
  });
});

describe("snapshot-hydrated documents replay history for concurrent imports", () => {
  test("List and MovableList", () => {
    for (const kind of ["List", "MovableList"] as const) {
      const get = (target: LoroDoc): LoroList | LoroMovableList =>
        kind === "List" ? target.getList("list") : target.getMovableList("list");
      const a = doc(1);
      for (const value of ["a", "b", "c"]) get(a).push(value);
      a.commit();
      const c = doc(3);
      c.import(a.export({ mode: "update" }));
      get(a).delete(1, 1);
      a.commit();
      // Inserted after "b", which the snapshot below no longer contains.
      get(c).insert(2, "z");
      c.commit();

      const hydrated = new LoroDoc();
      hydrated.import(a.export({ mode: "snapshot" }));
      hydrated.import(c.export({ mode: "update" }));
      a.import(c.export({ mode: "update" }));
      expect(hydrated.toJSON()).toEqual(a.toJSON());
      expect(hydrated.toJSON()).toEqual({ list: ["a", "z", "c"] });
    }
  });

  test("the import's event comes from its ops, as in Rust", () => {
    const b = doc(2);
    const list = b.getMovableList("list");
    list.insertContainer(0, new LoroText());
    list.insert(1, "v8");
    b.commit();
    const c = doc(3);
    c.import(b.export({ mode: "update" }));
    list.insert(2, 12);
    list.move(0, 2);
    b.commit();
    const hydrated = new LoroDoc();
    hydrated.import(b.export({ mode: "snapshot" }));
    // Concurrent with the snapshot: "v8" moves, but stays at index 0.
    c.getMovableList("list").move(1, 0);
    c.commit();

    const events: LoroEventBatch[] = [];
    hydrated.subscribe((event) => events.push(event));
    hydrated.import(c.export({ mode: "update" }));
    expect(hydrated.toJSON()).toEqual({ list: ["v8", 12, ""] });
    expect(events.map(({ events }) => events.map(({ diff }) => diff))).toEqual([
      [{ type: "list", diff: [{ delete: 1 }, { insert: ["v8"] }] }],
    ]);
  });
});

describe("shallow imports", () => {
  test("a root at the empty version accepts changes without dependencies", () => {
    const empty = doc(1);
    const shallow = new LoroDoc();
    shallow.import(empty.export({ mode: "shallow-snapshot", frontiers: [] }));
    const other = doc(2);
    other.getMovableList("list").push("x");
    other.commit();
    shallow.import(other.export({ mode: "update" }));
    expect(shallow.toJSON()).toEqual({ list: ["x"] });
  });

  test("changes already inside the shallow root are skipped", () => {
    const source = doc(3);
    source.getMovableList("list").push("v");
    source.commit();
    const shallow = new LoroDoc();
    shallow.import(
      source.export({ mode: "shallow-snapshot", frontiers: source.frontiers() }),
    );
    shallow.import(source.export({ mode: "snapshot" }));
    expect(shallow.toJSON()).toEqual({ list: ["v"] });
  });

  test("the part of a change after the root must not depend on trimmed history", () => {
    const a = doc(3);
    a.getMovableList("list").push("a");
    a.commit();
    const b = doc(1);
    b.import(a.export({ mode: "update" }));
    b.getMovableList("list").push("x");
    b.commit();
    const shallow = new LoroDoc();
    shallow.import(b.export({ mode: "shallow-snapshot", frontiers: b.frontiers() }));
    // One change holds `a` (before the root) and `b`, which depends on `a`.
    a.getMovableList("list").push("b");
    a.commit();
    expect(a.getAllChanges().get("3")).toHaveLength(1);
    expect(() => shallow.import(a.export({ mode: "update" }))).toThrow(/outdated/u);
    expect(shallow.toJSON()).toEqual({ list: ["a", "x"] });
  });

  test("a root at a peer's first op rejects only changes without dependencies", () => {
    const a = doc(1);
    a.getMovableList("list").push("x");
    a.commit();
    const shallow = new LoroDoc();
    shallow.import(a.export({ mode: "shallow-snapshot", frontiers: a.frontiers() }));
    const other = doc(2);
    other.getMovableList("list").push("y");
    other.commit();
    expect(() => shallow.import(other.export({ mode: "update" }))).toThrow(/outdated/u);
    const next = doc(4);
    next.import(a.export({ mode: "update" }));
    next.getMovableList("list").push("z");
    next.commit();
    shallow.import(next.export({ mode: "update" }));
    expect(shallow.toJSON()).toEqual({ list: ["x", "z"] });
  });

  test("writes a zero start-version entry for a root at a peer's first op", () => {
    const source = doc(3);
    source.getMovableList("list").push("v");
    source.commit();
    const bytes = source.export({
      mode: "shallow-snapshot",
      frontiers: source.frontiers(),
    });
    const entries = decodeSstable(decodeFastSnapshot(bytes).oplog);
    const startVersion = entries.find(
      ({ key }) => new TextDecoder().decode(key) === "sv",
    )!.value;
    // postcard: one entry, peer 3, counter 0.
    expect([...startVersion]).toEqual([1, 3, 0]);
  });
});
