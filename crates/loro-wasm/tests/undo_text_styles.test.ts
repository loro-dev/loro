// Undo/redo of text marks next to a collaborator's edits (loro-dev/loro#1229,
// loro-dev/loro#1009). See context/undo-text-styles.md.
import { describe, expect, test } from "vitest";
import { LoroDoc, UndoManager } from "../bundler/index";

function styledDoc(peer: number, key: string): LoroDoc {
  const doc = new LoroDoc();
  doc.setPeerId(peer);
  doc.configTextStyle({ [key]: { expand: "after" } });
  return doc;
}

function sync(from: LoroDoc, to: LoroDoc) {
  to.import(from.export({ mode: "update", from: to.oplogVersion() }));
}

describe("undo of text marks next to collaborators", () => {
  test("undo keeps a collaborator's later value for the same key (#1229)", () => {
    const a = styledDoc(1, "color");
    const b = styledDoc(2, "color");
    a.getText("text").insert(0, "x");
    a.commit();
    sync(a, b);
    const undo = new UndoManager(a, { mergeInterval: 0 });
    a.getText("text").mark({ start: 0, end: 1 }, "color", "red");
    a.commit();
    sync(a, b);
    b.getText("text").mark({ start: 0, end: 1 }, "color", "blue");
    b.commit();
    sync(b, a);

    const blue = [{ insert: "x", attributes: { color: "blue" } }];
    expect(a.getText("text").toDelta()).toEqual(blue);
    undo.undo();
    expect(a.getText("text").toDelta()).toEqual(blue);
    undo.redo();
    expect(a.getText("text").toDelta()).toEqual(blue);
  });

  test("redo keeps a collaborator's removal (#1229 case 2)", () => {
    const a = styledDoc(1, "color");
    const b = styledDoc(2, "color");
    a.getText("text").insert(0, "x");
    a.commit();
    sync(a, b);
    const undo = new UndoManager(a, { mergeInterval: 0 });
    a.getText("text").mark({ start: 0, end: 1 }, "color", "red");
    a.commit();
    sync(a, b);
    b.getText("text").unmark({ start: 0, end: 1 }, "color");
    b.commit();

    expect(undo.undo()).toBe(true);
    sync(b, a);
    undo.redo();
    expect(a.getText("text").toDelta()).toEqual([{ insert: "x" }]);
  });

  test.each([
    ["ab", "x"],
    ["😀a😀", "🎉x🎉"],
  ])(
    "undo removes the style when only inherited text remains (%s, %s)",
    (original, inserted) => {
      const a = styledDoc(1, "bold");
      const b = styledDoc(2, "bold");
      const undo = new UndoManager(a, { mergeInterval: 0 });
      a.getText("text").insert(0, original);
      a.commit();
      sync(a, b);
      // UTF-16 lengths: an emoji counts 2.
      const len = a.getText("text").length;
      a.getText("text").mark({ start: 0, end: len }, "bold", true);
      a.commit();
      sync(a, b);
      const textB = b.getText("text");
      const first = [...original][0].length;
      textB.insert(first, inserted);
      textB.delete(0, first);
      textB.delete(inserted.length, len - first);
      b.commit();
      sync(b, a);

      const bold = [{ insert: inserted, attributes: { bold: true } }];
      const plain = [{ insert: inserted }];
      expect(a.getText("text").toDelta()).toEqual(bold);
      expect(undo.undo()).toBe(true);
      expect(a.getText("text").toDelta()).toEqual(plain);
      expect(undo.redo()).toBe(true);
      expect(a.getText("text").toDelta()).toEqual(bold);
      expect(undo.undo()).toBe(true);
      sync(a, b);
      expect(b.getText("text").toDelta()).toEqual(plain);
    },
  );

  test("undo removes the style from a concurrent insert it absorbed (#1009)", () => {
    for (const [peerA, peerB] of [
      [1, 2],
      [2, 1],
    ]) {
      const a = styledDoc(peerA, "bold");
      const b = styledDoc(peerB, "bold");
      const undo = new UndoManager(a, { mergeInterval: 0 });
      a.getText("text").insert(0, "important note");
      a.commit();
      sync(a, b);
      a.getText("text").mark({ start: 0, end: 9 }, "bold", true);
      a.commit();
      b.getText("text").insert(0, "very ");
      b.commit();
      sync(b, a);

      expect(undo.undo()).toBe(true);
      expect(a.getText("text").toDelta()).toEqual([
        { insert: "very important note" },
      ]);
    }
  });
});
