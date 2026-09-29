import { describe, expect, test } from "vitest";

import { LoroDoc, type Frontiers, type LoroTree, type TreeID } from "../src/index";

/**
 * Tree diffs apply in order, as in Rust: each index refers to the tree that
 * the earlier items leave. Expected shapes match Rust main (WASM build).
 */
type Shape = [string | null, Shape[]];

const shape = (doc: LoroDoc): Shape[] => {
  const walk = (
    nodes: readonly { meta?: { name?: string }; children?: unknown[] }[],
  ): Shape[] =>
    nodes.map((node) => [
      node.meta?.name ?? null,
      walk((node.children ?? []) as { meta?: { name?: string } }[]),
    ]);
  return walk(doc.getTree("t").toJSON() as { meta?: { name?: string } }[]);
};

function newDoc(): { doc: LoroDoc; tree: LoroTree } {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  const tree = doc.getTree("t");
  tree.enableFractionalIndex(0);
  return { doc, tree };
}

function named(tree: LoroTree, name: string, parent?: TreeID, index?: number): TreeID {
  const node =
    parent === undefined
      ? tree.createNode(undefined, index)
      : tree.getNodeByID(parent)!.createNode(index);
  node.data.set("name", name);
  return node.id;
}

/** The state at `to`, reached by applyDiff on a fork at `from`. */
function expectApplyReaches(
  doc: LoroDoc,
  from: Frontiers,
  to: Frontiers,
  expected: Shape[],
): void {
  doc.checkout(to);
  expect(shape(doc)).toEqual(expected);
  doc.checkoutToLatest();
  const fork = doc.forkAt(from);
  fork.setDetachedEditing(true);
  fork.applyDiff(doc.diff(from, to, false));
  expect(shape(fork)).toEqual(expected);
}

/** revertTo(`to`) on a copy of `doc` reaches `expected`. */
function expectRevertReaches(doc: LoroDoc, to: Frontiers, expected: Shape[]): void {
  const copy = new LoroDoc();
  copy.import(doc.export({ mode: "snapshot" }));
  copy.setPeerId(5);
  copy.revertTo(to);
  copy.commit();
  expect(shape(copy)).toEqual(expected);
}

describe("tree diffs apply in order", () => {
  test("revertTo moves a node out of a subtree before deleting it", () => {
    // Round-6 review NB1: A deletes p; B concurrently moves c out of p and
    // creates g under c; A then moves x into g. Reverting to `target` used to
    // delete everything first and throw at `create p` on an empty root list,
    // leaving an empty tree.
    const a = new LoroDoc();
    a.setPeerId(1);
    const tree = a.getTree("t");
    tree.enableFractionalIndex(0);
    const p = named(tree, "p");
    const c = named(tree, "c", p);
    a.commit();
    const x = named(tree, "x", undefined, 0);
    a.commit();
    const target = a.frontiers();
    const y = named(tree, "y", undefined, 2);
    a.commit();
    const b = new LoroDoc();
    b.setPeerId(2);
    b.import(a.export({ mode: "snapshot" }));
    tree.delete(p);
    a.commit();
    b.getTree("t").move(c, undefined, 2);
    const g = named(b.getTree("t"), "g", c);
    b.commit();
    a.import(b.export({ mode: "update", from: a.oplogVersion() }));
    tree.move(x, g, 0);
    a.commit();

    const diff = a.diff(a.frontiers(), target, false);
    const items = diff.find(([id]) => id === tree.id)![1] as {
      diff: { action: string; target: string; index?: number; oldIndex?: number }[];
    };
    // x leaves g before g's ancestor c is deleted, and p is created after x
    // is back at the root, as in Rust.
    expect(items.diff.map((item) => [item.action, item.target])).toEqual([
      ["move", x],
      ["delete", y],
      ["delete", g],
      ["delete", c],
      ["create", p],
      ["create", c],
    ]);
    a.revertTo(target);
    a.commit();
    expect(shape(a)).toEqual([
      ["x", []],
      ["p", [["c", []]]],
    ]);
  });

  test("a node leaves a subtree the range deletes, next to a moved sibling", () => {
    const { doc, tree } = newDoc();
    const a = named(tree, "a");
    const b = named(tree, "b");
    doc.commit();
    const target = doc.frontiers();
    const c = named(tree, "c");
    tree.move(b);
    tree.move(a, c);
    for (const name of ["d", "e", "f"]) named(tree, name);
    doc.commit();
    const expected: Shape[] = [
      ["a", []],
      ["b", []],
    ];
    expectApplyReaches(doc, doc.frontiers(), target, expected);
    expectRevertReaches(doc, target, expected);
  });

  test("a move into a parent created and deleted in the range removes the node", () => {
    const { doc, tree } = newDoc();
    named(tree, "a");
    const b = named(tree, "b");
    doc.commit();
    const from = doc.frontiers();
    const p = named(tree, "p");
    tree.move(b, p);
    tree.delete(p);
    doc.commit();
    expectApplyReaches(doc, from, doc.frontiers(), [["a", []]]);
  });

  test("a node hidden under a deleted node inside a new subtree is removed", () => {
    const { doc, tree } = newDoc();
    const a = named(tree, "a");
    const b = named(tree, "b");
    doc.commit();
    const from = doc.frontiers();
    const r = named(tree, "r");
    tree.move(a, b);
    tree.move(b, r);
    tree.delete(b);
    doc.commit();
    expectApplyReaches(doc, from, doc.frontiers(), [["r", []]]);
  });
});

describe("tree items that cannot apply reject the batch", () => {
  test("nothing is written when a later item fails", () => {
    const { doc, tree } = newDoc();
    const a = named(tree, "a");
    named(tree, "b");
    doc.commit();
    const before = doc.toJSON();
    for (const items of [
      // The create succeeds; the parent of the second never exists.
      [
        { action: "create", target: "8@9", index: 0, fractionalIndex: "80" },
        {
          action: "create",
          target: "9@9",
          parent: "7@9",
          index: 0,
          fractionalIndex: "80",
        },
      ],
      // The delete applies; the move then has too few siblings.
      [
        { action: "delete", target: a, oldIndex: 0 },
        { action: "move", target: "2@1", index: 1, fractionalIndex: "80", oldIndex: 1 },
      ],
      // A move below itself.
      [
        {
          action: "move",
          target: a,
          parent: a,
          index: 0,
          fractionalIndex: "80",
          oldIndex: 0,
        },
      ],
    ] as const) {
      expect(() =>
        doc.applyDiff([[tree.id, { type: "tree", diff: items as never }]]),
      ).toThrow(RangeError);
      expect(doc.toJSON()).toEqual(before);
      expect(doc.getPendingTxnLength()).toBe(0);
    }
  });
});

describe("deep trees", () => {
  /** Best of two runs of diff, import and checkout (with a subscriber) on an n-deep chain. */
  function deepChainMs(depth: number): number {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    let node = doc.getTree("t").createNode();
    for (let level = 0; level < depth; level += 1) {
      node = node.createNode();
      node.data.set("k", level);
    }
    doc.commit();
    const update = doc.export({ mode: "update" });
    let best = Infinity;
    for (let run = 0; run < 2; run += 1) {
      const started = performance.now();
      expect(doc.diff([], doc.frontiers(), true).length).toBeGreaterThan(depth);
      const replica = new LoroDoc();
      let events = 0;
      replica.subscribe(() => (events += 1));
      replica.import(update);
      const latest = replica.frontiers();
      replica.checkout([]);
      replica.checkout(latest);
      expect(events).toBe(3);
      best = Math.min(best, performance.now() - started);
    }
    return best;
  }

  test("diff, import and checkout with a subscriber stay linear in depth", () => {
    // Each node used to walk to the root to check whether it is hidden
    // (quadratic: 4x the depth took about 17x the time). Linear is about 5x.
    const ratio = deepChainMs(8_000) / deepChainMs(2_000);
    expect(ratio).toBeLessThan(12);
  });
});
