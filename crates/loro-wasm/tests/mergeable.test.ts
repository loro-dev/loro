import { describe, expect, test, vi } from "vitest";
import { LoroDoc, LoroMap, LoroText, UndoManager } from "../bundler/index";

function sync(a: LoroDoc, b: LoroDoc) {
  const aBytes = a.export({ mode: "update", from: b.version() });
  const bBytes = b.export({ mode: "update", from: a.version() });
  a.import(bBytes);
  b.import(aBytes);
}

function oneMs(): Promise<void> {
  return new Promise((r) => setTimeout(r));
}

describe("mergeable containers (WASM bindings)", () => {
  test("concurrent counter increments converge", () => {
    const a = new LoroDoc();
    const b = new LoroDoc();
    a.setPeerId("1");
    b.setPeerId("2");

    a.getMap("state").ensureMergeableCounter("revision").increment(1);
    b.getMap("state").ensureMergeableCounter("revision").increment(1);
    a.commit();
    b.commit();
    sync(a, b);

    expect(a.toJSON()).toEqual({ state: { revision: 2 } });
    expect(b.toJSON()).toEqual({ state: { revision: 2 } });
  });

  test("delete clears the discriminator; re-get resurfaces preserved state", () => {
    const doc = new LoroDoc();
    doc.setPeerId("1");
    const root = doc.getMap("state");
    const counter = root.ensureMergeableCounter("revision");
    counter.increment(3);
    doc.commit();
    expect(doc.toJSON()).toEqual({ state: { revision: 3 } });

    // delete clears the discriminator slot, exactly like a regular container delete.
    root.delete("revision");
    doc.commit();
    expect(doc.toJSON()).toEqual({ state: {} });

    // Re-get rewrites the discriminator (the slot is now empty), so the child resurfaces with
    // its preserved state (3), not a reset to 0.
    root.ensureMergeableCounter("revision");
    doc.commit();
    expect(doc.toJSON()).toEqual({ state: { revision: 3 } });

    // Further mutation accrues on the preserved state.
    root.ensureMergeableCounter("revision").increment(10);
    doc.commit();
    expect(doc.toJSON()).toEqual({ state: { revision: 13 } });
  });

  test("getContainerById resolves an ensured-but-empty mergeable child", () => {
    const doc = new LoroDoc();
    doc.setPeerId("1");
    const note = doc.getMap("records").ensureMergeableMap("note");

    // Resolvable right after ensure, before any op is written and before commit.
    expect(doc.toJSON()).toEqual({ records: { note: {} } });
    expect(doc.hasContainer(note.id)).toBe(true);
    const retrieved = doc.getContainerById(note.id);
    expect(retrieved).toBeDefined();
    expect(retrieved!.id).toBe(note.id);

    // A mergeable cid that was never ensured must not resolve.
    const phantom = doc
      .getMap("records")
      .ensureMergeableMap("note")
      .id.replace("note", "phantom");
    expect(doc.getContainerById(phantom as any)).toBeUndefined();

    // The ensured-but-empty state must also resolve by id on a remote peer.
    doc.commit();
    const peer = new LoroDoc();
    peer.setPeerId("2");
    peer.import(doc.export({ mode: "update" }));
    expect(peer.toJSON()).toEqual({ records: { note: {} } });
    expect(peer.getContainerById(note.id)).toBeDefined();
  });

  test("ensureMergeableMap, ensureMergeableList, ensureMergeableText smoke", () => {
    const doc = new LoroDoc();
    doc.setPeerId("1");
    const root = doc.getMap("state");

    root.ensureMergeableMap("nested").set("k", "v");
    root.ensureMergeableList("items").insert(0, 1);
    root.ensureMergeableText("body").insert(0, "hello");
    doc.commit();

    expect(doc.toJSON()).toEqual({
      state: {
        nested: { k: "v" },
        items: [1],
        body: "hello",
      },
    });
  });

  test("ensureMergeableMovableList: concurrent inserts converge and mov works end-to-end", () => {
    const a = new LoroDoc();
    const b = new LoroDoc();
    a.setPeerId("1");
    b.setPeerId("2");

    const aItems = a.getMap("state").ensureMergeableMovableList("items");
    const bItems = b.getMap("state").ensureMergeableMovableList("items");
    // Deterministic cid: both peers resolve to the same id.
    expect(aItems.id).toEqual(bItems.id);

    aItems.insert(0, "first");
    aItems.insert(1, "second");
    bItems.insert(0, "from_b");
    a.commit();
    b.commit();
    sync(a, b);

    const aValue = a.toJSON() as { state: { items: string[] } };
    expect(aValue.state.items).toHaveLength(3);
    expect(new Set(aValue.state.items)).toEqual(
      new Set(["first", "second", "from_b"]),
    );
    expect(b.toJSON()).toEqual(a.toJSON());

    // Exercise the MovableList-specific `move` operation through the handle.
    const aItemsAgain = a.getMap("state").ensureMergeableMovableList("items");
    const preMoveOrder = (a.toJSON() as { state: { items: string[] } }).state
      .items.slice();
    aItemsAgain.move(0, aItemsAgain.length - 1);
    a.commit();
    const postMoveOrder = (a.toJSON() as { state: { items: string[] } }).state
      .items;
    expect(postMoveOrder).not.toEqual(preMoveOrder);
    expect(new Set(postMoveOrder)).toEqual(new Set(preMoveOrder));
  });

  test("ensureMergeableTree: concurrent root creates converge", () => {
    const a = new LoroDoc();
    const b = new LoroDoc();
    a.setPeerId("1");
    b.setPeerId("2");

    const aTree = a.getMap("state").ensureMergeableTree("hierarchy");
    const bTree = b.getMap("state").ensureMergeableTree("hierarchy");
    // Deterministic cid: both peers resolve to the same id.
    expect(aTree.id).toEqual(bTree.id);

    const aRoot = aTree.createNode();
    aTree.createNode(aRoot.id);
    bTree.createNode();
    a.commit();
    b.commit();
    sync(a, b);

    // Both peers' root nodes survive on the merged tree.
    const aValue = a.toJSON() as { state: { hierarchy: unknown[] } };
    expect(aValue.state.hierarchy).toHaveLength(2);
    expect(b.toJSON()).toEqual(a.toJSON());
  });

  // Subscription-flush invariant (see AGENTS.md: "Flush Pending Events In `loro-wasm`").
  //
  // The six `ensureMergeable*` methods on `LoroMap` now emit a discriminator `MapSet` op against
  // the parent map (loro-dev/loro#759), which DOES produce a document-level event — exactly like
  // a plain `LoroMap.set`. Plus downstream mutations through the returned handle
  // (`counter.increment`, `tree.createNode`, list/text inserts) emit events too. All of these go
  // through the same auto-commit barrier as `LoroMap.set`, whose events are flushed by the
  // already-decorated `commit`. With an active subscription on the parent, calling
  // `ensureMergeable*` and then mutating must NOT leave any events on the JS pending queue past the
  // microtask boundary.
  //
  // This test asserts the contract holds: the subscription fires, AND no
  // "[LORO_INTERNAL_ERROR] Event not called" line is emitted.
  test("ensureMergeable* methods do not leave events pending under an active subscription", async () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      const doc = new LoroDoc();
      doc.setPeerId("1");

      let parentEvents = 0;
      doc.getMap("state").subscribe(() => {
        parentEvents += 1;
      });

      const root = doc.getMap("state");
      // Touch every `ensureMergeable*` flavor while a subscription is active. None of these
      // calls should leave pending events behind. The downstream mutations below force
      // event emission; the assertion is that all events were delivered cleanly via the
      // existing flush path (i.e. through `commit` already in the allowlist), with no
      // internal "Event not called" warning.
      const counter = root.ensureMergeableCounter("revision");
      const nested = root.ensureMergeableMap("nested");
      const list = root.ensureMergeableList("items");
      const movable = root.ensureMergeableMovableList("movable");
      const text = root.ensureMergeableText("body");
      const tree = root.ensureMergeableTree("hierarchy");

      counter.increment(1);
      nested.set("k", "v");
      list.insert(0, "x");
      movable.insert(0, "y");
      text.insert(0, "hello");
      tree.createNode();
      doc.commit();
      await oneMs();

      // Sanity: the parent subscription fired at least once for the mutations above.
      expect(parentEvents).toBeGreaterThan(0);
      // Invariant: the binding must not log the pending-events error.
      const sawInternalError = errorSpy.mock.calls.some((args) =>
        args.some((arg) =>
          String(arg).includes("[LORO_INTERNAL_ERROR] Event not called"),
        ),
      );
      expect(sawInternalError).toBe(false);
    } finally {
      errorSpy.mockRestore();
    }
  });
  test("revertTo / applyDiff restore a deleted mergeable child once", () => {
    const setup = () => {
      const d = new LoroDoc();
      d.setPeerId(1);
      const m = d.getMap("m");
      m.ensureMergeableText("t").insert(0, "hello");
      m.ensureMergeableCounter("c").increment(7);
      m.ensureMergeableList("l").push("keep");
      m.ensureMergeableMovableList("ml").push("keep");
      d.commit();
      const a = d.frontiers();
      for (const key of ["t", "c", "l", "ml"]) m.delete(key);
      d.commit();
      return { d, a, b: d.frontiers() };
    };
    const expected = { m: { t: "hello", c: 7, l: ["keep"], ml: ["keep"] } };

    const r = setup();
    r.d.revertTo(r.a);
    r.d.commit();
    expect(r.d.toJSON()).toEqual(expected);

    const p = setup();
    // The doc keeps the hidden children, so the full-state diff needs `fullState`.
    p.d.applyDiff(p.d.diff(p.b, p.a), { fullState: true });
    p.d.commit();
    expect(p.d.toJSON()).toEqual(expected);
  });
  describe("revert of hidden mergeable children that diverged from the target", () => {
    // Target: t="hello", l=["keep"], c=7, tree with one node. Then the hidden children
    // diverge (t="hello!", l=["keep","extra"], c=10, an extra tree node) and are deleted.
    const setup = (peer: number) => {
      const d = new LoroDoc();
      d.setPeerId(peer);
      const m = d.getMap("m");
      m.ensureMergeableText("t").insert(0, "hello");
      m.ensureMergeableList("l").push("keep");
      m.ensureMergeableCounter("c").increment(7);
      const tree = m.ensureMergeableTree("tree");
      const n = tree.createNode();
      n.data.set("v", "keep");
      d.commit();
      const target = d.frontiers();
      m.ensureMergeableText("t").insert(5, "!");
      m.ensureMergeableList("l").push("extra");
      m.ensureMergeableCounter("c").increment(3);
      tree.createNode();
      d.commit();
      for (const key of ["t", "l", "c", "tree"]) m.delete(key);
      d.commit();
      return { d, target, nodeId: n.id };
    };
    const expected = {
      t: "hello",
      l: ["keep"],
      c: 7,
    };
    const visible = (d: LoroDoc) => {
      const { tree, ...rest } = d.toJSON().m;
      return { rest, tree };
    };

    test("revertTo keeps tree node identity", () => {
      const { d, target, nodeId } = setup(1);
      d.revertTo(target);
      d.commit();
      const { rest, tree } = visible(d);
      expect(rest).toEqual(expected);
      expect(tree.map((n: any) => n.id)).toEqual([nodeId]);
    });

    test("local revert events forwarded through applyDiff match the source", () => {
      const { d, target } = setup(1);
      const mirror = d.fork();
      const batches: [string, any][][] = [];
      const unsub = d.subscribe((e) => {
        batches.push(e.events.map((ev) => [ev.target, ev.diff]));
      });
      d.revertTo(target);
      d.commit();
      unsub();
      for (const batch of batches) mirror.applyDiff(batch as any);
      mirror.commit();
      expect(mirror.toJSON()).toEqual(d.toJSON());
      expect(visible(mirror).rest).toEqual(expected);
    });

    test("two peers reverting concurrently merge to the target", () => {
      const { d: a, target, nodeId } = setup(1);
      const b = a.fork();
      b.setPeerId(2);
      a.revertTo(target);
      a.commit();
      b.revertTo(target);
      b.commit();
      sync(a, b);
      const { rest, tree } = visible(a);
      // The counter is additive: both peers compensate 10 -> 7, like a root counter.
      expect(rest).toEqual({ ...expected, c: 4 });
      expect(tree.map((n: any) => n.id)).toEqual([nodeId]);
      expect(b.toJSON()).toEqual(a.toJSON());
    });
  });
  test("diff() restores a deleted mergeable child on a doc that never saw it", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    const m = source.getMap("m");
    m.ensureMergeableText("t").insert(0, "hello");
    m.ensureMergeableCounter("c").increment(7);
    m.ensureMergeableList("l").push("keep");
    source.commit();
    const target = source.frontiers();
    m.ensureMergeableCounter("c").increment(3);
    for (const key of ["t", "c", "l"]) m.delete(key);
    source.commit();

    const mirror = new LoroDoc();
    mirror.getMap("m");
    mirror.applyDiff(source.diff(source.frontiers(), target));
    mirror.commit();
    expect(mirror.toJSON()).toEqual({ m: { t: "hello", c: 7, l: ["keep"] } });
  });

  test("a peer can undo its edit after a remote undo re-activates the child", () => {
    const a = new LoroDoc();
    a.setPeerId(1);
    const t = a.getMap("m").ensureMergeableText("s");
    t.insert(0, "hello");
    a.commit();
    const b = a.fork();
    b.setPeerId(2);
    const ua = new UndoManager(a, {});
    const ub = new UndoManager(b, {});
    a.getMap("m").delete("s");
    a.commit();
    b.getMap("m").ensureMergeableText("s").insert(5, "!");
    b.commit();
    a.import(b.export({ mode: "update" }));
    expect(ua.undo()).toBe(true);
    a.commit();
    b.import(a.export({ mode: "update" }));
    expect(b.toJSON()).toEqual({ m: { s: "hello!" } });
    expect(ub.canUndo()).toBe(true);
    expect(ub.undo()).toBe(true);
    b.commit();
    expect(b.toJSON()).toEqual({ m: { s: "hello" } });
  });
  test("applyDiff is incremental by default and aligns with fullState", () => {
    const setup = () => {
      const d = new LoroDoc();
      d.setPeerId(1);
      d.getMap("m").ensureMergeableText("t").insert(0, "hello");
      d.commit();
      const target = d.frontiers();
      d.getMap("m").delete("t");
      d.commit();
      return { d, diff: d.diff(d.frontiers(), target) };
    };
    // Same hidden state: the default applies the entry as an increment, as before.
    const a = setup();
    a.d.applyDiff(a.diff);
    a.d.commit();
    expect(a.d.toJSON()).toEqual({ m: { t: "hellohello" } });
    // With fullState it is aligned with the hidden state.
    const b = setup();
    b.d.applyDiff(b.diff, { fullState: true });
    b.d.commit();
    expect(b.d.toJSON()).toEqual({ m: { t: "hello" } });
    // A doc without hidden state gets the same result either way.
    for (const options of [undefined, { fullState: true }]) {
      const fresh = new LoroDoc();
      fresh.getMap("m");
      fresh.applyDiff(setup().diff, options);
      fresh.commit();
      expect(fresh.toJSON()).toEqual({ m: { t: "hello" } });
    }
    expect(() =>
      setup().d.applyDiff(setup().diff, { fullState: "yes" } as any),
    ).toThrow();
  });

  test("fullState applyDiff keeps reordered movable-list children", () => {
    for (const kind of ["map", "text"]) {
      const d = new LoroDoc();
      d.setPeerId(1);
      const list = d.getMap("m").ensureMergeableMovableList("s");
      for (const [i, v] of ["A", "B"].entries()) {
        if (kind === "map") list.insertContainer(i, new LoroMap()).set("v", v);
        else list.insertContainer(i, new LoroText()).insert(0, v);
      }
      d.commit();
      const target = d.frontiers();
      const expected = d.toJSON();
      list.move(0, 1);
      d.getMap("m").delete("s");
      d.commit();
      d.applyDiff(d.diff(d.frontiers(), target), { fullState: true });
      d.commit();
      expect(d.toJSON()).toEqual(expected);
    }
  });

  test("local re-ensure events mirror onto a doc sharing the hidden state", () => {
    const d = new LoroDoc();
    d.setPeerId(1);
    const m = d.getMap("m");
    m.ensureMergeableCounter("c").increment(7);
    m.ensureMergeableText("t").insert(0, "hello");
    m.ensureMergeableList("l").push("keep");
    d.commit();
    for (const key of ["c", "t", "l"]) m.delete(key);
    d.commit();
    const mirror = d.fork();
    const batches: [string, any][][] = [];
    const unsub = d.subscribe((e) => {
      batches.push(e.events.map((ev) => [ev.target, ev.diff]));
    });
    m.ensureMergeableCounter("c");
    m.ensureMergeableText("t");
    m.ensureMergeableList("l").push("more");
    d.commit();
    unsub();
    for (const batch of batches) mirror.applyDiff(batch as any);
    mirror.commit();
    const expected = { m: { c: 7, t: "hello", l: ["keep", "more"] } };
    expect(d.toJSON()).toEqual(expected);
    expect(mirror.toJSON()).toEqual(expected);
  });
});
