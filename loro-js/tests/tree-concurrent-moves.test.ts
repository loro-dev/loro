import { describe, expect, test } from "vitest";

import { LoroDoc, UndoManager } from "../src/index";
import { loadRustReference } from "./support/rust-reference";

const rust = loadRustReference();
type DocClass = typeof LoroDoc;

function crossingMoves(Doc: DocClass) {
  const base = new Doc();
  base.setPeerId(1n);
  const tree = base.getTree("tr");
  const a = tree.createNode().id;
  const b = tree.createNode().id;
  base.commit();
  const peers = [1n, 2n].map((id) => {
    const doc = new Doc();
    doc.setPeerId(id);
    doc.import(base.export({ mode: "update" }));
    return doc;
  });
  const [left, right] = peers as [LoroDoc, LoroDoc];
  left.getTree("tr").move(a, b);
  left.commit();
  right.getTree("tr").move(b, a);
  right.commit();
  return { left, right, a, b };
}

function shape(doc: LoroDoc): unknown {
  return doc
    .getTree("tr")
    .toJSON()
    .map(function node(n): unknown {
      return { id: n.id, children: n.children.map(node) };
    });
}

const nested = [{ id: "1@1", children: [{ id: "0@1", children: [] }] }];
const roots = [
  { id: "1@1", children: [] },
  { id: "0@1", children: [] },
];

for (const [name, Doc] of [
  ["loro.js", LoroDoc],
  ["loro-crdt", rust?.LoroDoc],
] as const) {
  describe.skipIf(Doc === undefined)(
    `${name}: crossing concurrent tree moves (#1182)`,
    () => {
      test("skips the cycle-closing move and never revives it after a later move", () => {
        const { left, right, a } = crossingMoves(Doc!);
        left.import(right.export({ mode: "update" }));
        expect(shape(left)).toEqual(nested);
        expect(left.getTree("tr").getNodeByID("1@1")!.getLastMoveId()).toEqual({
          peer: "1",
          counter: 1,
        });
        left.getTree("tr").move(a, undefined);
        left.commit();
        expect(shape(left)).toEqual(roots);
      });
    },
  );
}

// A later writer can arrive first. Rejecting only the incoming move would
// keep that writer instead of Rust's earlier move on the other node.
for (const [name, Doc] of [
  ["loro.js", LoroDoc],
  ["loro-crdt", rust?.LoroDoc],
] as const) {
  describe.skipIf(Doc === undefined)(
    `${name}: replay and checkout of crossing moves`,
    () => {
      test("converges in both import orders and keeps effective move IDs", () => {
        const { left, right } = crossingMoves(Doc!);
        const leftBytes = left.export({ mode: "update" });
        const rightBytes = right.export({ mode: "update" });
        left.import(rightBytes);
        right.import(leftBytes);
        expect(right.getTree("tr").toJSON()).toEqual(left.getTree("tr").toJSON());
        expect(shape(right)).toEqual(nested);
        expect(right.getTree("tr").getNodeByID("1@1")!.getLastMoveId()).toEqual({
          peer: "1",
          counter: 1,
        });
      });

      test("replays updates and snapshots before and after the later root move", () => {
        const { left, right, a } = crossingMoves(Doc!);
        left.import(right.export({ mode: "update" }));
        for (const expected of [nested, roots]) {
          for (const mode of ["update", "snapshot"] as const) {
            const replay = new Doc!();
            replay.import(left.export({ mode }));
            expect(shape(replay)).toEqual(expected);
            expect(replay.getTree("tr").getNodeByID("1@1")!.getLastMoveId()).toEqual({
              peer: "1",
              counter: 1,
            });
            replay.getTree("tr").move(a, undefined);
            replay.commit();
            expect(shape(replay)).toEqual(roots);
          }
          left.getTree("tr").move(a, undefined);
          left.commit();
        }
      });

      test("checkout and diff re-evaluate skipped moves for each version", () => {
        const { left, right, a } = crossingMoves(Doc!);
        const beforeMerge = left.frontiers();
        const otherBranch = right.frontiers();
        left.import(right.export({ mode: "update" }));
        const merged = left.frontiers();
        left.getTree("tr").move(a, undefined);
        left.commit();
        const latest = left.frontiers();
        for (let repeat = 0; repeat < 2; repeat += 1) {
          left.checkout(otherBranch);
          expect(shape(left)).toEqual([
            { id: "0@1", children: [{ id: "1@1", children: [] }] },
          ]);
          left.checkout(merged);
          expect(shape(left)).toEqual(nested);
          left.checkout(beforeMerge);
          expect(shape(left)).toEqual(nested);
          left.checkout(latest);
          expect(shape(left)).toEqual(roots);
        }
        left.attach();
        for (const [from, to] of [
          [latest, merged],
          [otherBranch, latest],
          [merged, otherBranch],
        ]) {
          const diff = left.diff(from!, to!, false);
          const copy = new Doc!();
          copy.import(left.export({ mode: "snapshot" }));
          copy.checkout(from!);
          const expected = new Doc!();
          expected.import(left.export({ mode: "snapshot" }));
          expected.checkout(to!);
          copy.attach();
          copy.revertTo(from!);
          copy.applyDiff(diff);
          copy.commit();
          expect(shape(copy)).toEqual(shape(expected));
          expect(shape(left)).toEqual(roots);
        }
      });
    },
  );
}

describe.skipIf(rust === undefined)(
  "Rust binary interoperability for crossing moves",
  () => {
    test("imports Rust updates, JSON, snapshots and shallow snapshots", () => {
      const { left, right, a } = crossingMoves(rust!.LoroDoc);
      left.import(right.export({ mode: "update" }));
      const merged = left.frontiers();
      for (const expected of [nested, roots]) {
        for (const mode of ["update", "snapshot", "shallow-snapshot"] as const) {
          const replay = new LoroDoc();
          replay.import(
            left.export(
              mode === "shallow-snapshot" ? { mode, frontiers: merged } : { mode },
            ),
          );
          expect(shape(replay)).toEqual(expected);
          replay.getTree("tr").move(a, undefined);
          replay.commit();
          expect(shape(replay)).toEqual(roots);
        }
        const json = new LoroDoc();
        json.importJsonUpdates(left.exportJsonUpdates());
        expect(shape(json)).toEqual(expected);
        left.getTree("tr").move(a, undefined);
        left.commit();
      }
    });

    test("Rust reads loro.js exports and later moves identically", () => {
      const { left, right, a } = crossingMoves(LoroDoc);
      left.import(right.export({ mode: "update" }));
      const merged = left.frontiers();
      for (const expected of [nested, roots]) {
        for (const mode of ["update", "snapshot", "shallow-snapshot"] as const) {
          const replay = new rust!.LoroDoc();
          replay.import(
            left.export(
              mode === "shallow-snapshot" ? { mode, frontiers: merged } : { mode },
            ),
          );
          expect(shape(replay)).toEqual(expected);
          replay.getTree("tr").move(a, undefined);
          replay.commit();
          expect(shape(replay)).toEqual(roots);
        }
        left.getTree("tr").move(a, undefined);
        left.commit();
      }
    });

    test("batch and detached imports resolve to the same reachable tree", () => {
      const { left, right, a } = crossingMoves(rust!.LoroDoc);
      const updates = [left.export({ mode: "update" }), right.export({ mode: "update" })];
      for (const order of [updates, [...updates].reverse()]) {
        const batch = new LoroDoc();
        batch.importBatch(order);
        expect(shape(batch)).toEqual(nested);
        batch.getTree("tr").move(a, undefined);
        batch.commit();
        expect(shape(batch)).toEqual(roots);
        const detached = new LoroDoc();
        detached.import(order[0]!);
        detached.detach();
        detached.import(order[1]!);
        detached.attach();
        expect(shape(detached)).toEqual(nested);
      }
    });
  },
);

describe("effective move history", () => {
  test.skipIf(rust === undefined)(
    "undo restores the last effective move, not the skipped move",
    () => {
      for (const [Doc, Undo] of [
        [LoroDoc, UndoManager],
        [rust!.LoroDoc, rust!.UndoManager],
      ] as const) {
        const { left, right, a, b } = crossingMoves(Doc);
        left.import(right.export({ mode: "update" }));
        left.getTree("tr").move(a, undefined);
        left.commit();
        const undo = new Undo(left, { mergeInterval: 0 });
        left.getTree("tr").move(b, a);
        left.commit();
        undo.undo();
        expect(shape(left)).toEqual(roots);
        undo.redo();
        expect(shape(left)).toEqual([{ id: a, children: [{ id: b, children: [] }] }]);
        undo.clear();
      }
    },
  );

  test("import events include the previously applied move that becomes skipped", () => {
    const { left, right, a } = crossingMoves(LoroDoc);
    const mirror = new LoroDoc();
    mirror.import(right.export({ mode: "snapshot" }));
    right.subscribe((batch) => {
      mirror.applyDiff(batch.events.map(({ target, diff }) => [target, diff]));
      mirror.commit();
    });
    right.import(left.export({ mode: "update" }));
    expect(shape(mirror)).toEqual(nested);
    right.getTree("tr").move(a, undefined);
    right.commit();
    expect(shape(mirror)).toEqual(roots);
  });

  test("a failed import drops cached moves before their op IDs are reused", () => {
    const { left, right } = crossingMoves(LoroDoc);
    const list = right.getMovableList("list");
    for (const value of ["a", "b", "c"]) list.push(value);
    list.move(0, 2);
    right.commit();
    const forged = right.exportJsonUpdates();
    for (const change of forged.changes) {
      for (const op of change.ops) {
        if (op.container === "cid:root-tr:Tree" && op.content.type === "move") {
          (op.content as { parent: string | null }).parent = null;
        }
        if (op.container === "cid:root-list:MovableList" && op.content.type === "move") {
          (op.content as { from: number }).from = 9;
        }
      }
    }
    const before = left.toJSON();
    const version = left.oplogVersion();
    expect(() => left.importJsonUpdates(forged)).toThrow(/out of range/u);
    expect(left.toJSON()).toEqual(before);
    expect(left.oplogVersion().toJSON()).toEqual(version.toJSON());
    left.import(right.export({ mode: "update" }));
    expect(shape(left)).toEqual(nested);
    expect(left.getTree("tr").getNodeByID("1@1")!.getLastMoveId()).toEqual({
      peer: "1",
      counter: 1,
    });
  });

  test.skipIf(rust === undefined)(
    "three-peer cycles converge in all six delivery orders",
    () => {
      const base = new LoroDoc();
      base.setPeerId(1);
      const ids = Array.from({ length: 3 }, () => base.getTree("tr").createNode().id);
      base.commit();
      const peers = ids.map((id, index) => {
        const doc = new LoroDoc();
        doc.setPeerId(index + 1);
        doc.import(base.export({ mode: "update" }));
        doc.getTree("tr").move(id, ids[(index + 1) % ids.length]);
        doc.commit();
        return doc.export({ mode: "update" });
      });
      for (const order of [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
      ]) {
        const js = new LoroDoc();
        const reference = new rust!.LoroDoc();
        for (const peer of order) {
          js.import(peers[peer]!);
          reference.import(peers[peer]!);
          expect(js.getTree("tr").toJSON()).toEqual(reference.getTree("tr").toJSON());
        }
        const merged = js.frontiers();
        js.getTree("tr").move(ids[0]!, undefined);
        reference.getTree("tr").move(ids[0]!, undefined);
        js.commit();
        reference.commit();
        expect(js.getTree("tr").toJSON()).toEqual(reference.getTree("tr").toJSON());
        js.checkout(merged);
        reference.checkout(merged);
        expect(js.getTree("tr").toJSON()).toEqual(reference.getTree("tr").toJSON());
      }
    },
  );
});

test.skipIf(rust === undefined)(
  "orders interleaved operations within concurrent changes",
  () => {
    for (const Doc of [LoroDoc, rust!.LoroDoc]) {
      const { left, right, a } = crossingMoves(Doc);
      // The root move has not seen the concurrent B→A move. Its lamport is
      // later, so B→A must be checked (and skipped) before this root move.
      left.getTree("tr").move(a, undefined);
      left.commit();
      const updates = [left.export({ mode: "update" }), right.export({ mode: "update" })];
      for (const bytes of [updates, [...updates].reverse()]) {
        const replay = new LoroDoc();
        for (const update of bytes) replay.import(update);
        expect(shape(replay)).toEqual(roots);
        expect(replay.getTree("tr").getNodeByID("1@1")!.getLastMoveId()).toEqual({
          peer: "1",
          counter: 1,
        });
      }
    }
  },
);
