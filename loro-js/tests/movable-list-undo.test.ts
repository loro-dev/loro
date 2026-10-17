import { describe, expect, test } from "vitest";
import * as js from "../src/index";
import { loadRustReference } from "./support/rust-reference";

const rust = loadRustReference();

for (const [name, engine] of [
  ["loro.js", js],
  ["Rust", rust],
] as const) {
  describe.skipIf(engine === undefined)(`${name} MovableList undo identities`, () => {
    const { LoroDoc, UndoManager, LoroText, LoroMovableList } = engine!;
    const pair = (values: number[]) => {
      const a = new LoroDoc();
      a.setPeerId(1);
      values.forEach((value, i) => a.getMovableList("list").insert(i, value));
      a.commit();
      const b = new LoroDoc();
      b.setPeerId(2);
      b.import(a.export({ mode: "update" }));
      const undo = new UndoManager(a, { mergeInterval: 0 });
      return {
        a,
        b,
        undo,
        list: a.getMovableList("list"),
        remote: b.getMovableList("list"),
      };
    };
    const sync = (a: js.LoroDoc, b: js.LoroDoc) => {
      b.import(a.export({ mode: "update" }));
      a.import(b.export({ mode: "update" }));
      expect(a.toJSON()).toEqual(b.toJSON());
    };

    test("move and set undo write one native op with the same element ID", () => {
      const { a, undo, list } = pair([0, 1, 2]);
      list.move(0, 2);
      a.commit();
      const forward = a
        .exportJsonUpdates()
        .changes.flatMap((c) => c.ops)
        .at(-1)!.content;
      const before = a.oplogVersion();
      expect(undo.undo()).toBe(true);
      const inverse = a.exportJsonUpdates(before).changes.flatMap((c) => c.ops);
      expect(inverse).toHaveLength(1);
      expect(inverse[0]!.content).toMatchObject({
        type: "move",
        elem_id: (forward as { elem_id: string }).elem_id,
      });
      expect(list.toArray()).toEqual([0, 1, 2]);
      list.set(0, 9);
      a.commit();
      const setVersion = a.oplogVersion();
      expect(undo.undo()).toBe(true);
      const setInverse = a.exportJsonUpdates(setVersion).changes.flatMap((c) => c.ops);
      expect(setInverse).toHaveLength(1);
      expect(setInverse[0]!.content).toMatchObject({
        type: "set",
        elem_id: (forward as { elem_id: string }).elem_id,
        value: 0,
      });
    });

    test("concurrent moves preserve identity even for equal scalar values", () => {
      const { a, b, undo, list, remote } = pair([7, 7, 7]);
      list.move(0, 2);
      a.commit();
      remote.move(0, 1);
      b.commit();
      sync(a, b);
      undo.undo();
      expect(list.toArray()).toEqual([7, 7, 7]);
      // B still addresses the original element without receiving the undo.
      remote.set(1, 99);
      b.commit();
      sync(a, b);
      expect(list.toArray()).toEqual([99, 7, 7]);
    });

    test.each(["moved", "deleted", "crossed", "inserted"] as const)(
      "removal undo follows %s neighbours",
      (kind) => {
        const { a, b, undo, list, remote } = pair([0, 1, 2, 3]);
        list.delete(1, 1);
        a.commit();
        if (kind === "moved") remote.move(0, 1);
        if (kind === "deleted") remote.delete(0, 1);
        if (kind === "crossed") remote.move(0, 3);
        if (kind === "inserted") remote.insert(1, 9);
        b.commit();
        sync(a, b);
        const removed = list.toArray();
        const expected = {
          moved: [0, 1, 2, 3],
          deleted: [1, 2, 3],
          crossed: [2, 3, 0, 1],
          inserted: [0, 1, 9, 2, 3],
        }[kind];
        for (let i = 0; i < 3; i += 1) {
          undo.undo();
          expect(list.toArray()).toEqual(expected);
          sync(a, b);
          undo.redo();
          expect(list.toArray()).toEqual(removed);
          sync(a, b);
        }
      },
    );

    test.each(["move", "set"] as const)(
      "undo of %s does not revive a remote deletion",
      (kind) => {
        const { a, b, undo, list, remote } = pair([0, 1, 2]);
        if (kind === "move") list.move(0, 2);
        else list.set(0, 9);
        a.commit();
        b.import(a.export({ mode: "update" }));
        remote.delete(kind === "move" ? 2 : 0, 1);
        b.commit();
        sync(a, b);
        const version = a.oplogVersion().toJSON();
        expect(undo.undo()).toBe(false);
        expect(a.oplogVersion().toJSON()).toEqual(version);
        expect(list.toArray()).toEqual([1, 2]);
        expect(undo.canUndo()).toBe(false);
        expect(undo.canRedo()).toBe(false);
        sync(a, b);
      },
    );

    test("an inverse insertion delete follows the moved identity", () => {
      const { a, b, undo, list, remote } = pair([7, 7]);
      list.insert(1, 7);
      a.commit();
      b.import(a.export({ mode: "update" }));
      remote.move(1, 2);
      b.commit();
      sync(a, b);
      undo.undo();
      remote.set(0, 10);
      remote.set(1, 20);
      b.commit();
      sync(a, b);
      expect(list.toArray()).toEqual([10, 20]);
    });

    test("earlier insertion undo finds an identity recreated by removal undo", () => {
      const { a, undo, list } = pair([0, 1]);
      list.insert(1, 7);
      a.commit();
      list.delete(1, 1);
      a.commit();
      for (let i = 0; i < 3; i += 1) {
        undo.undo();
        expect(list.toArray()).toEqual([0, 7, 1]);
        undo.undo();
        expect(list.toArray()).toEqual([0, 1]);
        undo.redo();
        expect(list.toArray()).toEqual([0, 7, 1]);
        undo.redo();
        expect(list.toArray()).toEqual([0, 1]);
      }
    });

    test("removal anchors follow a neighbour recreated by an earlier undo", () => {
      const { a, b, undo, list, remote } = pair([0, 1, 2, 3]);
      list.delete(1, 1);
      a.commit();
      list.delete(0, 1);
      a.commit();
      remote.move(2, 0);
      b.commit();
      sync(a, b);
      for (let i = 0; i < 3; i += 1) {
        undo.undo();
        expect(list.toArray()).toEqual([0, 2, 3]);
        undo.undo();
        expect(list.toArray()).toEqual([0, 1, 2, 3]);
        sync(a, b);
        undo.redo();
        expect(list.toArray()).toEqual([0, 2, 3]);
        undo.redo();
        expect(list.toArray()).toEqual([2, 3]);
        sync(a, b);
      }
    });

    test("one undo item can span an unrelated remote import", () => {
      const { a, b, undo, list } = pair([0, 1, 2, 3]);
      undo.groupStart();
      list.delete(1, 1);
      a.commit();
      b.getMap("remote").set("unrelated", true);
      b.commit();
      a.import(b.export({ mode: "update" }));
      list.move(0, 2);
      a.commit();
      undo.groupEnd();
      undo.undo();
      expect(list.toArray()).toEqual([0, 1, 2, 3]);
      expect(a.getMap("remote").get("unrelated")).toBe(true);
      undo.redo();
      expect(list.toArray()).toEqual([2, 3, 0]);
      sync(a, b);
    });

    test.each(["same-element", "neighbour-move", "neighbour-insert"] as const)(
      "identity replay after an excluded %s matches Rust",
      (kind) => {
        const a = new LoroDoc();
        a.setPeerId(1);
        const list = a.getMovableList("list");
        const initial = kind === "same-element" ? ["X", "A", "B"] : ["A", "B", "C", "D"];
        for (const value of initial)
          list.insertContainer(list.length, new LoroText()).insert(0, value);
        a.commit();
        const undo = new UndoManager(a, {
          mergeInterval: 0,
          excludeOriginPrefixes: ["system"],
        });
        list.move(0, kind === "same-element" ? 2 : 1);
        a.commit();
        if (kind === "same-element") list.move(2, 1);
        if (kind === "neighbour-move") list.move(0, 3);
        if (kind === "neighbour-insert") list.insert(0, "new");
        a.commit({ origin: "system" });
        expect(undo.undo()).toBe(true);
        expect(list.toJSON()).toEqual(
          {
            "same-element": ["X", "A", "B"],
            "neighbour-move": ["C", "D", "A", "B"],
            "neighbour-insert": ["new", "A", "B", "C", "D"],
          }[kind],
        );
      },
    );

    test("move undo after snapshot hydration keeps the original identity", () => {
      const { a } = pair([0, 1, 2]);
      const restored = new LoroDoc();
      restored.setPeerId(3);
      restored.import(a.export({ mode: "snapshot" }));
      const undo = new UndoManager(restored, { mergeInterval: 0 });
      const list = restored.getMovableList("list");
      list.move(0, 2);
      restored.commit();
      undo.undo();
      expect(list.toArray()).toEqual([0, 1, 2]);
      a.getMovableList("list").set(0, 99);
      a.commit();
      sync(restored, a);
      expect(list.toArray()).toEqual([99, 1, 2]);
    });

    test("a recreated list child keeps aliases for an older insertion undo", () => {
      const a = new LoroDoc();
      a.setPeerId(1);
      const parent = a.getMovableList("parent");
      const child = parent.insertContainer(0, new LoroMovableList());
      child.insert(0, 0);
      a.commit();
      const undo = new UndoManager(a, { mergeInterval: 0 });
      child.insert(1, 1);
      a.commit();
      parent.delete(0, 1);
      a.commit();
      for (let i = 0; i < 3; i += 1) {
        undo.undo();
        expect(parent.toJSON()).toEqual([[0, 1]]);
        undo.undo();
        expect(parent.toJSON()).toEqual([[0]]);
        undo.redo();
        expect(parent.toJSON()).toEqual([[0, 1]]);
        undo.redo();
        expect(parent.toJSON()).toEqual([]);
      }
    });

    test("a child moved concurrently keeps its ID and contents", () => {
      const { a, b, list, remote } = pair([0, 1]);
      const child = list.insertContainer(0, new LoroText());
      child.insert(0, "content");
      a.commit();
      sync(a, b);
      const undo = new UndoManager(a, { mergeInterval: 0 });
      list.move(0, 2);
      a.commit();
      remote.move(0, 1);
      b.commit();
      sync(a, b);
      undo.undo();
      expect((list.get(0) as js.LoroText).id).toBe(child.id);
      expect(list.toJSON()).toEqual(["content", 0, 1]);
      sync(a, b);
    });
  });
}
