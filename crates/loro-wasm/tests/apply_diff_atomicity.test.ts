import { describe, expect, it } from "vitest";
import { ContainerID, LoroDoc, UndoManager } from "../bundler/index";

// `applyDiff` is all or nothing: a batch with an invalid entry throws and
// leaves the doc, its history and its subscribers as they were
// (loro-dev/loro#1154).

function failingBatch(): [ContainerID, unknown][] {
  return [
    // Applied first, so partial application would be visible
    ["cid:root-text:Text", { type: "text", diff: [{ insert: "partial" }] }],
    ["cid:root-list:List", { type: "list", diff: [{ delete: 5 }] }],
  ];
}

function snapshot(doc: LoroDoc) {
  return {
    value: doc.toJSON(),
    version: doc.version().toJSON(),
    frontiers: doc.frontiers(),
    changes: doc.changeCount(),
    ops: doc.opCount(),
  };
}

describe("applyDiff atomicity", () => {
  it("throws and leaves the doc unchanged when an entry is out of bounds", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    doc.getList("list").push(1);
    doc.commit();
    const before = snapshot(doc);

    expect(() => doc.applyDiff(failingBatch() as never)).toThrow();
    doc.commit();
    expect(snapshot(doc)).toStrictEqual(before);

    // Still usable; the next ops reuse the rolled back batch's op ids.
    doc.getText("text").insert(0, "ok");
    doc.commit();
    expect(doc.toJSON()).toStrictEqual({ list: [1], text: "ok" });
    const replay = new LoroDoc();
    replay.import(doc.export({ mode: "update" }));
    expect(replay.toJSON()).toStrictEqual(doc.toJSON());
  });

  it("emits no event and adds no undo step for a failed batch", async () => {
    const doc = new LoroDoc();
    const undo = new UndoManager(doc, {});
    doc.getList("list").push(1);
    doc.commit();
    let events = 0;
    doc.subscribe(() => {
      events += 1;
    });
    let updates = 0;
    doc.subscribeLocalUpdates(() => {
      updates += 1;
    });

    expect(() => doc.applyDiff(failingBatch() as never)).toThrow();
    doc.commit();
    await Promise.resolve();
    expect(events).toBe(0);
    expect(updates).toBe(0);
    // The only undo step is the push.
    expect(undo.undo()).toBe(true);
    expect(doc.toJSON()).toStrictEqual({ list: [] });
    expect(undo.canUndo()).toBe(false);
  });

  it("keeps uncommitted edits made before a failed batch", () => {
    const doc = new LoroDoc();
    doc.getList("list").push(1);
    doc.commit();
    doc.getText("mine").insert(0, "abc");
    expect(() => doc.applyDiff(failingBatch() as never)).toThrow();
    doc.commit();
    expect(doc.toJSON()).toStrictEqual({ list: [1], mine: "abc" });
  });

  it("rolls back a stale diff made for another version", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    const text = source.getText("text");
    text.insert(0, "hello world");
    source.commit();
    const v1 = source.frontiers();
    const map = source.getMap("map");
    map.set("k", 1);
    text.delete(0, 6);
    source.getList("list").insert(0, "x");
    source.commit();
    const diff = source.diff(v1, source.frontiers(), false);

    // Not at v1: the text is too short for the delete.
    const target = new LoroDoc();
    target.getText("text").insert(0, "hi");
    target.getList("list").push(1);
    target.commit();
    const before = snapshot(target);
    expect(() => target.applyDiff(diff)).toThrow();
    target.commit();
    expect(snapshot(target)).toStrictEqual(before);
  });
});
