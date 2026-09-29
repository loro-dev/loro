import { describe, expect, test } from "vitest";

import { LoroDoc } from "../src/index";
import type { LoroTree } from "../src/index";

// Expected values are what loro-crdt returns for the same calls.

function shape(tree: LoroTree): string[] {
  return tree.toJSON().map((node) => `${node.id}:${node.fractional_index}`);
}

/** Two roots created concurrently at index 0 share the position 80. */
function equalPositions(): LoroDoc {
  const p1 = new LoroDoc();
  p1.setPeerId(1);
  const p2 = new LoroDoc();
  p2.setPeerId(2);
  p1.getTree("tree").createNode();
  p1.commit();
  p2.getTree("tree").createNode();
  p2.commit();
  p1.import(p2.export({ mode: "update" }));
  expect(shape(p1.getTree("tree"))).toEqual(["0@1:80", "0@2:80"]);
  return p1;
}

describe("local tree positions", () => {
  // Rust's generate_fi_at: there is no index between two equal positions, so
  // the right neighbor is moved to a new position in the same transaction.
  test("rearranges equal neighbors when creating a node between them", () => {
    const doc = equalPositions();
    const tree = doc.getTree("tree");
    const before = doc.opCount();
    const node = tree.createNode(undefined, 1);
    doc.commit();
    expect(node.id).toBe("1@1");
    expect(doc.opCount() - before).toBe(2);
    expect(shape(tree)).toEqual(["0@1:80", "1@1:817F80", "0@2:8180"]);

    const replay = new LoroDoc();
    replay.import(doc.export({ mode: "update" }));
    expect(shape(replay.getTree("tree"))).toEqual(shape(tree));
  });

  test("rearranges equal neighbors when moving a node between them", () => {
    const doc = equalPositions();
    const tree = doc.getTree("tree");
    const extra = tree.createNode();
    doc.commit();
    const before = doc.opCount();
    tree.move(extra.id, undefined, 1);
    doc.commit();
    expect(doc.opCount() - before).toBe(2);
    expect(shape(tree)).toEqual(["0@1:80", "1@1:817F80", "0@2:8180"]);
  });

  test("records nothing for a move to the current position", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const tree = doc.getTree("tree");
    const a = tree.createNode();
    const b = tree.createNode();
    const c = tree.createNode();
    doc.commit();
    const count = (edit: () => void): number => {
      const before = doc.opCount();
      edit();
      doc.commit();
      return doc.opCount() - before;
    };

    expect(count(() => tree.move(a.id, undefined, 0))).toBe(0);
    // Without an index the node goes last; it already is.
    expect(count(() => tree.move(c.id, undefined))).toBe(0);
    expect(count(() => c.move(undefined))).toBe(0);
    expect(count(() => b.moveAfter(a))).toBe(0);
    expect(count(() => a.moveBefore(b))).toBe(0);
    expect(tree.toJSON().map((node) => node.id)).toEqual([a.id, b.id, c.id]);
  });

  // Rust's mov_after/mov_before count the index without the moved node.
  test("moves after and before a sibling in the same parent", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const tree = doc.getTree("tree");
    const a = tree.createNode();
    const b = tree.createNode();
    const c = tree.createNode();
    doc.commit();

    a.moveAfter(c);
    doc.commit();
    expect(tree.toJSON().map((node) => node.id)).toEqual([b.id, c.id, a.id]);
    a.moveBefore(b);
    doc.commit();
    expect(tree.toJSON().map((node) => node.id)).toEqual([a.id, b.id, c.id]);
    b.moveAfter(c);
    doc.commit();
    expect(tree.toJSON().map((node) => node.id)).toEqual([a.id, c.id, b.id]);
  });

  test("moves within a parent in time independent of the sibling count", () => {
    /** Best of three runs of 1,000 same-parent moves, half of them no-ops. */
    function movesMs(siblings: number): number {
      const doc = new LoroDoc();
      doc.setPeerId(1);
      const tree = doc.getTree("tree");
      const nodes = Array.from({ length: siblings }, () => tree.createNode());
      doc.commit();
      let best = Infinity;
      for (let run = 0; run < 3; run += 1) {
        const started = performance.now();
        for (let step = 0; step < 500; step += 1) {
          const node = nodes[(step * 7_919) % siblings]!;
          tree.move(node.id, undefined, node.index()!);
          tree.move(node.id, undefined, 0);
        }
        doc.commit();
        best = Math.min(best, performance.now() - started);
      }
      return best;
    }
    // Counting the siblings by listing them made each move linear in their
    // number (16x the siblings took about 16x the time).
    const ratio = movesMs(16_000) / movesMs(1_000);
    expect(ratio).toBeLessThan(4);
  }, 60_000);

  // Rust's is_ancestor_of treats a deleted node's parent as the deleted root.
  test("moves a node under its own deleted child", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const tree = doc.getTree("tree");
    const parent = tree.createNode();
    const child = tree.createNode(parent.id);
    doc.commit();
    tree.delete(child.id);
    doc.commit();
    const before = doc.opCount();
    tree.move(parent.id, child.id, 0);
    doc.commit();
    expect(doc.opCount() - before).toBe(1);
    expect(tree.toJSON()).toEqual([]);
    expect(() => tree.move(child.id, child.id, 0)).toThrow(/below itself/);
  });
});
