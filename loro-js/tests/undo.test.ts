import { Loro, UndoManager } from "../src";
import { describe, expect, test } from "vitest";

describe("undo", () => {
  test("basic text undo", () => {
    const doc = new Loro();
    doc.setPeerId(1);
    const undo = new UndoManager(doc, { maxUndoSteps: 100, mergeInterval: 0 });
    expect(undo.canRedo()).toBeFalsy();
    expect(undo.canUndo()).toBeFalsy();
    doc.getText("text").insert(0, "hello");
    doc.commit();
    doc.getText("text").insert(5, " world!");
    doc.commit();
    expect(undo.canRedo()).toBeFalsy();
    expect(undo.canUndo()).toBeTruthy();
    undo.undo();
    expect(undo.canRedo()).toBeTruthy();
    expect(undo.canUndo()).toBeTruthy();
    expect(doc.toJSON()).toStrictEqual({
      text: "hello",
    });
    undo.undo();
    expect(undo.canRedo()).toBeTruthy();
    expect(undo.canUndo()).toBeFalsy();
    expect(doc.toJSON()).toStrictEqual({
      text: "",
    });
    undo.redo();
    expect(undo.canRedo()).toBeTruthy();
    expect(undo.canUndo()).toBeTruthy();
    expect(doc.toJSON()).toStrictEqual({
      text: "hello",
    });
    undo.redo();
    expect(undo.canRedo()).toBeFalsy();
    expect(undo.canUndo()).toBeTruthy();
    expect(doc.toJSON()).toStrictEqual({
      text: "hello world!",
    });
  });

  test("merge", async () => {
    const doc = new Loro();
    const undo = new UndoManager(doc, { maxUndoSteps: 100, mergeInterval: 50 });
    for (let i = 0; i < 10; i++) {
      doc.getText("text").insert(i, i.toString());
      doc.commit();
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
    for (let i = 0; i < 10; i++) {
      doc.getText("text").insert(i, i.toString());
      doc.commit();
    }
    expect(doc.toJSON()).toStrictEqual({
      text: "01234567890123456789",
    });
    undo.undo();
    expect(doc.toJSON()).toStrictEqual({
      text: "0123456789",
    });
    undo.undo();
    expect(doc.toJSON()).toStrictEqual({
      text: "",
    });
  });

  test("max undo steps", () => {
    const doc = new Loro();
    const undo = new UndoManager(doc, { maxUndoSteps: 100, mergeInterval: 0 });
    for (let i = 0; i < 200; i++) {
      doc.getText("text").insert(0, "0");
      doc.commit();
    }
    expect(doc.getText("text").length).toBe(200);
    while (undo.canUndo()) {
      undo.undo();
    }
    expect(doc.getText("text").length).toBe(100);
  });
});
