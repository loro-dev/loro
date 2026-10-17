import { expect, test } from "vitest";

import { LoroDoc, LoroMap, UndoManager } from "../src/index";

test("no-op undo returns false and preserves the earlier parent item", () => {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  const popped: boolean[] = [];
  const undo = new UndoManager(doc, {
    mergeInterval: 0,
    onPop: (isUndo) => popped.push(isUndo),
  });
  const root = doc.getMap("root");
  root.setContainer("parent", new LoroMap()).set("value", 42);
  doc.commit();
  const publicEvents: unknown[] = [];
  doc.subscribe((event) => publicEvents.push(event));

  // A recorded item can have an empty inverse even without remote edits.
  const list = doc.getMovableList("list");
  list.push("temporary");
  list.delete(0, 1);
  doc.commit();
  const before = doc.toJSON();
  const version = doc.version();
  const frontiers = doc.frontiers();

  expect(undo.undo()).toBe(false);
  expect(doc.toJSON()).toEqual(before);
  expect(doc.version()).toEqual(version);
  expect(doc.frontiers()).toEqual(frontiers);
  expect(publicEvents).toEqual([]);
  expect(popped).toEqual([true]);
  expect(undo.canUndo()).toBe(true);
  expect(undo.canRedo()).toBe(false);
  expect(undo.undo()).toBe(true);
  expect(root.get("parent")).toBeUndefined();
  expect(undo.canUndo()).toBe(false);
  expect(undo.canRedo()).toBe(true);
});

test("no-op redo returns false and preserves the later parent item", () => {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  const list = doc.getMovableList("list");
  list.push(1);
  list.push(2);
  doc.commit();
  const popped: boolean[] = [];
  const undo = new UndoManager(doc, {
    mergeInterval: 0,
    onPop: (isUndo) => popped.push(isUndo),
  });
  list.move(0, 1);
  doc.commit();
  const root = doc.getMap("root");
  root.setContainer("parent", new LoroMap());
  doc.commit();
  expect(undo.undo()).toBe(true);
  expect(undo.undo()).toBe(true);

  const remote = new LoroDoc();
  remote.setPeerId(2);
  remote.import(doc.export({ mode: "snapshot" }));
  // Redo moves a missing identity; a moved insertion is a real delete now.
  remote.getMovableList("list").delete(0, 1);
  remote.commit();
  doc.import(remote.export({ mode: "update" }));
  const before = doc.toJSON();
  const version = doc.version();
  const frontiers = doc.frontiers();

  expect(undo.redo()).toBe(false);
  expect(doc.toJSON()).toEqual(before);
  expect(doc.version()).toEqual(version);
  expect(doc.frontiers()).toEqual(frontiers);
  expect(popped).toEqual([true, true, false]);
  expect(undo.canRedo()).toBe(true);
  expect(undo.canUndo()).toBe(false);
  expect(undo.redo()).toBe(true);
  expect(root.get("parent")).toBeDefined();
  expect(undo.canRedo()).toBe(false);
  expect(undo.undo()).toBe(true);
  expect(doc.toJSON()).toEqual(before);
});

test("destroy stops recording no-op commits while public subscribers remain", () => {
  const doc = new LoroDoc();
  const publicEvents: unknown[] = [];
  const pushed: unknown[] = [];
  doc.subscribe((event) => publicEvents.push(event));
  const undo = new UndoManager(doc, {
    mergeInterval: 0,
    onPush: () => {
      pushed.push("item");
      return { value: null, cursors: [] };
    },
  });
  undo.destroy();
  const list = doc.getMovableList("list");
  list.push("temporary");
  list.delete(0, 1);
  doc.commit();

  expect(pushed).toEqual([]);
  expect(publicEvents).toEqual([]);
  expect(undo.canUndo()).toBe(false);
});
