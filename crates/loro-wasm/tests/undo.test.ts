import { Cursor, LoroDoc, UndoManager } from "../bundler/index";
import { describe, expect, test } from "vitest";

describe("undo", () => {
  test("basic text undo", () => {
    const doc = new LoroDoc();
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
    const doc = new LoroDoc();
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
    const doc = new LoroDoc();
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

  test("Skip chosen events", () => {
    const doc = new LoroDoc();
    const undo = new UndoManager(doc, {
      maxUndoSteps: 100,
      mergeInterval: 0,
      excludeOriginPrefixes: ["sys:"],
    });
    doc.getText("text").insert(0, "hello");
    doc.commit();
    doc.getText("text").insert(0, "1");
    doc.commit({ origin: "sys:test" });
    doc.getText("text").insert(2, "2");
    doc.commit({ origin: "sys:test" });
    doc.getText("text").insert(4, "3");
    doc.commit({ origin: "sys:test" });
    doc.getText("text").insert(8, " world!");
    doc.commit();
    doc.getText("text").insert(0, "Alice ");
    doc.commit();
    expect(doc.toJSON()).toStrictEqual({
      text: "Alice 1h2e3llo world!",
    });
    undo.undo();
    expect(doc.toJSON()).toStrictEqual({
      text: "1h2e3llo world!",
    });
    undo.undo();
    expect(doc.toJSON()).toStrictEqual({
      text: "1h2e3llo",
    });
    undo.undo();
    expect(doc.toJSON()).toStrictEqual({
      text: "123",
    });
    expect(undo.canUndo()).toBeFalsy();
    undo.redo();
    expect(doc.toJSON()).toStrictEqual({
      text: "1h2e3llo",
    });
    undo.redo();
    expect(doc.toJSON()).toStrictEqual({
      text: "1h2e3llo world!",
    });
    expect(undo.redo()).toBeTruthy();
    expect(doc.toJSON()).toStrictEqual({
      text: "Alice 1h2e3llo world!",
    });
    expect(undo.redo()).toBeFalsy();
  });

  test("undo event's origin", async () => {
    const doc = new LoroDoc();
    let undoing = false;
    let ran = false;
    doc.subscribe((e) => {
      if (undoing) {
        expect(e.origin).toBe("undo");
        ran = true;
      }
    });

    const undo = new UndoManager(doc, {});
    doc.getText("text").insert(0, "hello");
    doc.commit();
    await new Promise((r) => setTimeout(r, 10));
    undoing = true;
    undo.undo();
    await new Promise((r) => setTimeout(r, 10));
    expect(ran).toBeTruthy();
  });

  test("undo event listener", async () => {
    const doc = new LoroDoc();
    let pushReturn: null | number = null;
    let expectedValue: null | number = null;

    let pushTimes = 0;
    let popTimes = 0;
    const undo = new UndoManager(doc, {
      mergeInterval: 0,
      onPop: (isUndo, value, counterRange) => {
        expect(value.value).toBe(expectedValue);
        expect(value.cursors).toStrictEqual([]);
        popTimes += 1;
      },
      onPush: (isUndo, counterRange) => {
        pushTimes += 1;
        return { value: pushReturn, cursors: [] };
      },
    });

    doc.getText("text").insert(0, "hello");
    pushReturn = 1;
    doc.commit();
    doc.getText("text").insert(5, " world");
    pushReturn = 2;
    doc.commit();
    doc.getText("text").insert(0, "alice ");
    pushReturn = 3;
    doc.commit();
    expect(pushTimes).toBe(3);
    expect(popTimes).toBe(0);

    expectedValue = 3;
    undo.undo();
    expect(pushTimes).toBe(4);
    expect(popTimes).toBe(1);

    expectedValue = 2;
    undo.undo();
    expect(pushTimes).toBe(5);
    expect(popTimes).toBe(2);

    expectedValue = 1;
    undo.undo();
    expect(pushTimes).toBe(6);
    expect(popTimes).toBe(3);
  });

  test("undo cursor transform", async () => {
    const doc = new LoroDoc();
    let cursors: Cursor[] = [];
    let poppedCursors: Cursor[] = [];
    const undo = new UndoManager(doc, {
      mergeInterval: 0,
      onPop: (isUndo, value, counterRange) => {
        poppedCursors = value.cursors
      },
      onPush: () => {
        return { value: null, cursors: cursors };
      }
    });

    doc.getText("text").insert(0, "hello world");
    doc.commit();
    cursors = [
      doc.getText("text").getCursor(0)!,
      doc.getText("text").getCursor(5)!,
    ];
    doc.getText("text").delete(0, 6);
    doc.commit();
    expect(poppedCursors.length).toBe(0);
    undo.undo();
    expect(poppedCursors.length).toBe(2);
    expect(doc.toJSON()).toStrictEqual({
      text: "hello world",
    });
    expect(doc.getCursorPos(poppedCursors[0]).offset).toBe(0);
    expect(doc.getCursorPos(poppedCursors[1]).offset).toBe(5);
  });

  test("it can retrieve event in onPush event", async () => {
    const doc = new LoroDoc();
    let ran = false;
    const undo = new UndoManager(doc, {
      mergeInterval: 0,
      onPush: (isUndo, counterRange, event) => {
        expect(event).toBeDefined();
        expect(event?.by).toBe("local");
        expect(event?.origin).toBe("test");
        ran = true;
        return { value: null, cursors: [] };
      }
    });

    doc.getText("text").insert(0, "hello");
    doc.commit({ origin: "test" });
    await new Promise((r) => setTimeout(r, 1));
    expect(ran).toBeTruthy();
  })

  test('should automatically push to undo stack', async () => {
    const doc = new LoroDoc();
    let counter = 0;
    new UndoManager(doc, {
      onPush: () => {
        counter += 1;
        return { value: null, cursors: [] };
      }
    });

    doc.getText("text").insert(0, "hello");
    doc.commit();
    expect(counter).toBe(1);

    doc.getText("text").insert(0, "world");
    doc.commit();
    expect(counter).toBe(2);
  });
});
