import { describe, expect, test, vi } from "vitest";
import { LoroDoc } from "../bundler/index";

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
});
