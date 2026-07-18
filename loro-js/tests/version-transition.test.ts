import { describe, expect, test, vi } from "vitest";

import { LoroDoc } from "../src/runtime/document";
import type { LoroEventBatch } from "../src/runtime/types";

describe("indexed version transitions", () => {
  test("retreats and restores indexed container state", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const text = doc.getText("text");
    const list = doc.getList("list");
    const map = doc.getMap("map");
    const tree = doc.getTree("tree");
    const counter = doc.getCounter("counter");
    text.insert(0, "abcdef");
    list.insert(0, "a");
    list.insert(1, "b");
    map.set("value", 1);
    map.set("removed", true);
    const firstRoot = tree.createNode();
    const deletedRoot = tree.createNode();
    counter.increment(2);
    doc.commit();
    const baseFrontiers = doc.frontiers();
    const base = doc.toJSON();

    text.delete(1, 2);
    text.insert(2, "XY");
    list.delete(0, 1);
    list.insert(1, "c");
    map.set("value", 2);
    map.delete("removed");
    const child = tree.createNode(firstRoot.id);
    tree.delete(deletedRoot.id);
    counter.increment(3);
    doc.commit();
    const latest = doc.toJSON();

    for (let iteration = 0; iteration < 3; iteration += 1) {
      doc.checkout(baseFrontiers);
      expect(doc.toJSON()).toEqual(base);
      expect(tree.has(child.id)).toBe(false);
      expect(tree.has(deletedRoot.id)).toBe(true);
      doc.checkoutToLatest();
      expect(doc.toJSON()).toEqual(latest);
      expect(tree.has(child.id)).toBe(true);
      expect(tree.isNodeDeleted(deletedRoot.id)).toBe(true);
    }
  });

  test("switches between concurrent indexed branches", () => {
    const seed = new LoroDoc();
    seed.setPeerId(1);
    seed.getText("text").insert(0, "base");
    seed.getMap("map").set("base", true);
    seed.commit();
    const snapshot = seed.export({ mode: "snapshot" });

    const left = LoroDoc.fromSnapshot(snapshot);
    left.setPeerId(2);
    left.getText("text").insert(0, "L");
    left.getMap("map").set("left", true);
    left.getCounter("counter").increment(2);
    left.getTree("tree").createNode();
    left.commit();
    const leftFrontiers = left.frontiers();
    const leftValue = left.toJSON();

    const right = LoroDoc.fromSnapshot(snapshot);
    right.setPeerId(3);
    right.getText("text").push("R");
    right.getMap("map").set("right", true);
    right.getCounter("counter").increment(3);
    right.getTree("tree").createNode();
    right.commit();
    const rightFrontiers = right.frontiers();
    const rightValue = right.toJSON();

    left.import(right.export({ mode: "update", from: left.oplogVersion() }));
    const merged = left.toJSON();
    left.checkout(leftFrontiers);
    expect(left.toJSON()).toEqual(leftValue);
    left.checkout(rightFrontiers);
    expect(left.toJSON()).toEqual(rightValue);
    left.checkoutToLatest();
    expect(left.toJSON()).toEqual(merged);
  });

  test("matches a rebuilt fork across conflicting concurrent versions", () => {
    const seed = new LoroDoc();
    seed.setPeerId(1);
    const text = seed.getText("text");
    text.insert(0, "abc");
    seed.getList("list").push("base");
    seed.getMap("map").set("value", 0);
    const firstRoot = seed.getTree("tree").createNode();
    const secondRoot = seed.getTree("tree").createNode();
    seed.getCounter("counter").increment(1);
    seed.commit();
    const baseFrontiers = seed.frontiers();

    const left = seed.fork();
    left.setPeerId(2);
    left.getText("text").delete(1, 1);
    left.getText("text").insert(1, "L");
    left.getList("list").push("left");
    left.getMap("map").set("value", 1);
    left.getTree("tree").move(secondRoot.id, firstRoot.id);
    left.getCounter("counter").increment(2);
    left.commit();
    const leftFrontiers = left.frontiers();

    const right = seed.fork();
    right.setPeerId(3);
    right.getText("text").delete(1, 1);
    right.getText("text").insert(2, "R");
    right.getList("list").insert(0, "right");
    right.getMap("map").set("value", 2);
    right.getTree("tree").delete(secondRoot.id);
    right.getCounter("counter").increment(3);
    right.commit();
    const rightFrontiers = right.frontiers();

    left.import(right.export({ mode: "update", from: seed.oplogVersion() }));
    const mergedFrontiers = left.frontiers();
    const resets = [
      left.getText("text"),
      left.getList("list"),
      left.getMap("map"),
      left.getTree("tree"),
      left.getCounter("counter"),
    ].map((container) => vi.spyOn(container, "_reset"));

    for (const frontiers of [
      leftFrontiers,
      baseFrontiers,
      rightFrontiers,
      mergedFrontiers,
      baseFrontiers,
      mergedFrontiers,
    ]) {
      const expected = left.forkAt(frontiers);
      left.checkout(frontiers);
      expect(left.toJSON()).toEqual(expected.toJSON());
      expect(left.getText("text").toDelta()).toEqual(expected.getText("text").toDelta());
    }
    expect(resets.every((reset) => reset.mock.calls.length === 0)).toBe(true);
  });

  test("emits a compact replayable diff without resetting indexed containers", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const text = doc.getText("text");
    const list = doc.getList("list");
    const map = doc.getMap("map");
    const tree = doc.getTree("tree");
    const counter = doc.getCounter("counter");
    text.insert(0, "a".repeat(16_384));
    list.push("a");
    list.push("b");
    map.set("value", 1);
    const root = tree.createNode();
    counter.increment(1);
    doc.commit();
    const baseFrontiers = doc.frontiers();

    text.delete(8_192, 1);
    text.insert(8_192, "X");
    list.delete(0, 1);
    list.push("c");
    map.set("value", 2);
    tree.createNode(root.id);
    counter.increment(2);
    doc.commit();

    const latestMirror = doc.fork();
    let batch: LoroEventBatch | undefined;
    doc.subscribe((event) => {
      batch = event;
    });
    const resets = [text, list, map, tree, counter].map((container) =>
      vi.spyOn(container, "_reset"),
    );

    doc.checkout(baseFrontiers);
    expect(resets.every((reset) => reset.mock.calls.length === 0)).toBe(true);
    expect(batch).toBeDefined();
    latestMirror.applyDiff(batch!.events.map(({ target, diff }) => [target, diff]));
    expect(latestMirror.toJSON()).toEqual(doc.toJSON());
    const textDiff = batch!.events.find(({ target }) => target === text.id)?.diff;
    expect(JSON.stringify(textDiff).length).toBeLessThan(256);

    const baseMirror = doc.forkAt(baseFrontiers);
    batch = undefined;
    doc.checkoutToLatest();
    expect(resets.every((reset) => reset.mock.calls.length === 0)).toBe(true);
    expect(batch).toBeDefined();
    baseMirror.applyDiff(
      batch!.events
        .filter(({ target }) => target !== tree.id)
        .map(({ target, diff }) => [target, diff]),
    );
    expect(baseMirror.getText("text").toDelta()).toEqual(text.toDelta());
    expect(baseMirror.getList("list").toJSON()).toEqual(list.toJSON());
    expect(baseMirror.getMap("map").toJSON()).toEqual(map.toJSON());
    expect(baseMirror.getCounter("counter").value).toBe(counter.value);
    expect(batch!.events.find(({ target }) => target === tree.id)?.diff.type).toBe(
      "tree",
    );
  });

  test("calculates a retreat diff incrementally and restores the current state", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const text = doc.getText("text");
    const list = doc.getList("list");
    const map = doc.getMap("map");
    const counter = doc.getCounter("counter");
    text.insert(0, "a".repeat(16_384));
    list.push("base");
    map.set("value", 1);
    counter.increment(1);
    doc.commit();
    const baseFrontiers = doc.frontiers();

    text.delete(8_192, 1);
    text.insert(8_192, "X");
    list.push("latest");
    map.set("value", 2);
    counter.increment(2);
    doc.commit();
    const latestFrontiers = doc.frontiers();
    const latest = doc.toJSON();
    const mirror = doc.fork();
    const expectedBase = doc.forkAt(baseFrontiers);
    const resets = [text, list, map, counter].map((container) =>
      vi.spyOn(container, "_reset"),
    );

    const reverse = doc.diff(latestFrontiers, baseFrontiers, false);
    expect(resets.every((reset) => reset.mock.calls.length === 0)).toBe(true);
    expect(doc.toJSON()).toEqual(latest);
    expect(JSON.stringify(reverse).length).toBeLessThan(512);
    mirror.applyDiff(reverse);
    expect(mirror.getText("text").toDelta()).toEqual(
      expectedBase.getText("text").toDelta(),
    );
    expect(mirror.getList("list").toJSON()).toEqual(
      expectedBase.getList("list").toJSON(),
    );
    expect(mirror.getMap("map").toJSON()).toEqual(expectedBase.getMap("map").toJSON());
    expect(mirror.getCounter("counter").value).toBe(
      expectedBase.getCounter("counter").value,
    );
  });

  test("retreats and reapplies a full-range sequence deletion", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const text = doc.getText("text");
    text.insert(0, "x".repeat(16_384));
    doc.commit();
    const beforeDelete = doc.frontiers();
    const visibleMirror = doc.fork();

    text.delete(0, text.length);
    doc.commit();
    const afterDelete = doc.frontiers();
    const deletedMirror = doc.fork();

    doc.checkout(beforeDelete);
    expect(text.toString()).toBe("x".repeat(16_384));
    doc.checkout(afterDelete);
    expect(text.toString()).toBe("");

    let batch: LoroEventBatch | undefined;
    doc.subscribe((event) => {
      batch = event;
    });
    doc.checkout(beforeDelete);
    expect(batch).toBeDefined();
    deletedMirror.applyDiff(batch!.events.map(({ target, diff }) => [target, diff]));
    expect(deletedMirror.getText("text").toString()).toBe(text.toString());

    batch = undefined;
    doc.checkout(afterDelete);
    expect(batch).toBeDefined();
    visibleMirror.applyDiff(batch!.events.map(({ target, diff }) => [target, diff]));
    expect(visibleMirror.getText("text").toString()).toBe(text.toString());
  });

  test("retreats and restores a full-range sequence insertion", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const text = doc.getText("text");
    const beforeInsert = doc.frontiers();
    const emptyMirror = doc.fork();
    text.insert(0, "x".repeat(16_384));
    doc.commit();
    const afterInsert = doc.frontiers();
    const visibleMirror = doc.fork();

    doc.checkout(beforeInsert);
    expect(text.toString()).toBe("");
    doc.checkout(afterInsert);
    expect(text.toString()).toBe("x".repeat(16_384));

    let batch: LoroEventBatch | undefined;
    doc.subscribe((event) => {
      batch = event;
    });
    doc.checkout(beforeInsert);
    expect(batch).toBeDefined();
    visibleMirror.applyDiff(batch!.events.map(({ target, diff }) => [target, diff]));
    expect(visibleMirror.getText("text").toString()).toBe(text.toString());

    batch = undefined;
    doc.checkout(afterInsert);
    expect(batch).toBeDefined();
    emptyMirror.applyDiff(batch!.events.map(({ target, diff }) => [target, diff]));
    expect(emptyMirror.getText("text").toString()).toBe(text.toString());
  });

  test("keeps a range hidden while another concurrent deletion is included", () => {
    const seed = new LoroDoc();
    seed.setPeerId(1);
    seed.getText("text").insert(0, "x".repeat(4_096));
    seed.commit();
    const baseFrontiers = seed.frontiers();

    const left = seed.fork();
    left.setPeerId(2);
    left.getText("text").delete(0, 4_096);
    left.commit();
    const leftFrontiers = left.frontiers();

    const right = seed.fork();
    right.setPeerId(3);
    right.getText("text").delete(0, 4_096);
    right.commit();
    const rightFrontiers = right.frontiers();

    left.import(right.export({ mode: "update", from: seed.oplogVersion() }));
    expect(left.getText("text").toString()).toBe("");
    left.checkout(rightFrontiers);
    expect(left.getText("text").toString()).toBe("");
    left.checkout(leftFrontiers);
    expect(left.getText("text").toString()).toBe("");
    left.checkout(baseFrontiers);
    expect(left.getText("text").toString()).toBe("x".repeat(4_096));
    left.checkoutToLatest();
    expect(left.getText("text").toString()).toBe("");
  });

  test("retreats rich-text marks through per-element style history", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const text = doc.getText("text");
    text.insert(0, "abcdef");
    text.mark({ start: 1, end: 5 }, "bold", true);
    doc.commit();
    const baseFrontiers = doc.frontiers();

    text.unmark({ start: 2, end: 4 }, "bold");
    text.insert(3, "X");
    text.mark({ start: 0, end: 3 }, "bold", true);
    doc.commit();
    const latestFrontiers = doc.frontiers();
    const reset = vi.spyOn(text, "_reset");

    for (const frontiers of [
      baseFrontiers,
      latestFrontiers,
      baseFrontiers,
      latestFrontiers,
    ]) {
      const expected = doc.forkAt(frontiers).getText("text").toDelta();
      doc.checkout(frontiers);
      expect(text.toDelta()).toEqual(expected);
    }
    expect(reset).not.toHaveBeenCalled();
  });

  test("emits replayable range-based rich-text mark transitions", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const text = doc.getText("text");
    text.insert(0, "a".repeat(16_384));
    doc.commit();
    const baseFrontiers = doc.frontiers();
    const baseMirror = doc.fork();

    text.mark({ start: 1_024, end: 15_360 }, "bold", true);
    doc.commit();
    const latestFrontiers = doc.frontiers();
    const latestMirror = doc.fork();
    let batch: LoroEventBatch | undefined;
    doc.subscribe((event) => {
      batch = event;
    });

    doc.checkout(baseFrontiers);
    expect(batch).toBeDefined();
    latestMirror.applyDiff(batch!.events.map(({ target, diff }) => [target, diff]));
    expect(latestMirror.getText("text").toDelta()).toEqual(text.toDelta());
    expect(JSON.stringify(batch!.events[0]!.diff).length).toBeLessThan(256);

    batch = undefined;
    doc.checkout(latestFrontiers);
    expect(batch).toBeDefined();
    baseMirror.applyDiff(batch!.events.map(({ target, diff }) => [target, diff]));
    expect(baseMirror.getText("text").toDelta()).toEqual(text.toDelta());
    expect(JSON.stringify(batch!.events[0]!.diff).length).toBeLessThan(256);
  });

  test("keeps a subscribed full-range delete and mark transition compact", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const text = doc.getText("text");
    text.insert(0, "x".repeat(16_384));
    doc.commit();
    const baseFrontiers = doc.frontiers();
    const mirror = doc.fork();

    text.mark({ start: 0, end: text.length }, "bold", true);
    text.delete(0, text.length);
    doc.commit();
    const latestFrontiers = doc.frontiers();
    doc.checkout(baseFrontiers);

    let batch: LoroEventBatch | undefined;
    doc.subscribe((event) => {
      batch = event;
    });
    doc.checkout(latestFrontiers);

    expect(batch).toBeDefined();
    expect(JSON.stringify(batch!.events[0]!.diff).length).toBeLessThan(256);
    mirror.applyDiff(batch!.events.map(({ target, diff }) => [target, diff]));
    expect(mirror.getText("text").toDelta()).toEqual(text.toDelta());
  });

  test("retreats movable-list value writes without rebuilding its order", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const list = doc.getMovableList("list");
    list.push("a");
    list.push("b");
    list.push("c");
    doc.commit();
    const baseFrontiers = doc.frontiers();

    list.set(1, "B");
    doc.commit();
    const middleFrontiers = doc.frontiers();
    list.set(1, "latest");
    doc.commit();
    const latestFrontiers = doc.frontiers();
    const reset = vi.spyOn(list, "_reset");

    for (const frontiers of [
      baseFrontiers,
      latestFrontiers,
      middleFrontiers,
      baseFrontiers,
      latestFrontiers,
    ]) {
      const expected = doc.forkAt(frontiers).getMovableList("list").toJSON();
      doc.checkout(frontiers);
      expect(list.toJSON()).toEqual(expected);
    }
    expect(reset).not.toHaveBeenCalled();
  });

  test("retreats a movable-list transaction that mixes move, delete, and insert", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const list = doc.getMovableList("list");
    for (const value of ["a", "b", "c", "d"]) list.push(value);
    doc.commit();
    const baseFrontiers = doc.frontiers();

    list.move(1, 3);
    list.delete(3, 1);
    list.insert(1, "x");
    doc.commit();
    const latestFrontiers = doc.frontiers();
    const reset = vi.spyOn(list, "_reset");

    for (const frontiers of [
      baseFrontiers,
      latestFrontiers,
      baseFrontiers,
      latestFrontiers,
    ]) {
      const expected = doc.forkAt(frontiers).getMovableList("list").toJSON();
      doc.checkout(frontiers);
      expect(list.toJSON()).toEqual(expected);
    }
    expect(reset).not.toHaveBeenCalled();
  });

  test("switches concurrent movable-list moves by replaying branch positions", () => {
    const seed = new LoroDoc();
    seed.setPeerId(1);
    const seedList = seed.getMovableList("list");
    for (const value of ["a", "b", "c", "d"]) seedList.push(value);
    seed.commit();

    const left = seed.fork();
    left.setPeerId(2);
    left.getMovableList("list").move(0, 2);
    left.commit();
    const leftFrontiers = left.frontiers();
    const leftValue = left.getMovableList("list").toJSON();

    const right = seed.fork();
    right.setPeerId(3);
    right.getMovableList("list").move(0, 1);
    right.commit();
    const rightFrontiers = right.frontiers();
    const rightValue = right.getMovableList("list").toJSON();

    left.import(right.export({ mode: "update", from: seed.oplogVersion() }));
    const mergedFrontiers = left.frontiers();
    const mergedValue = left.getMovableList("list").toJSON();
    const list = left.getMovableList("list");
    const reset = vi.spyOn(list, "_reset");

    for (const [frontiers, expected] of [
      [leftFrontiers, leftValue],
      [rightFrontiers, rightValue],
      [leftFrontiers, leftValue],
      [mergedFrontiers, mergedValue],
    ] as const) {
      left.checkout(frontiers);
      expect(list.toJSON()).toEqual(expected);
    }
    expect(reset).not.toHaveBeenCalled();

    const diffResetCalls = reset.mock.calls.length;
    const diff = left.diff(leftFrontiers, rightFrontiers, false);
    const mirror = left.forkAt(leftFrontiers);
    mirror.applyDiff(diff);
    expect(mirror.getMovableList("list").toJSON()).toEqual(rightValue);
    expect(reset).toHaveBeenCalledTimes(diffResetCalls);
  });

  test("retreats and reapplies a sequential movable-list move suffix", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const list = doc.getMovableList("list");
    for (const value of ["a", "b", "c", "d"]) list.push(value);
    doc.commit();
    const baseFrontiers = doc.frontiers();

    list.move(0, 3);
    doc.commit();
    const middleFrontiers = doc.frontiers();
    list.move(1, 0);
    doc.commit();
    const latestFrontiers = doc.frontiers();
    const reset = vi.spyOn(list, "_reset");

    for (const frontiers of [
      middleFrontiers,
      baseFrontiers,
      latestFrontiers,
      middleFrontiers,
      latestFrontiers,
    ]) {
      const expected = doc.forkAt(frontiers).getMovableList("list").toJSON();
      doc.checkout(frontiers);
      expect(list.toJSON()).toEqual(expected);
    }
    expect(reset).not.toHaveBeenCalled();
  });

  test("retreats a movable-list move suffix through recorded anchors", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const list = doc.getMovableList("list");
    for (const value of ["a", "b", "c", "d"]) list.push(value);
    doc.commit();
    const baseFrontiers = doc.frontiers();

    list.move(0, 2);
    doc.commit();
    const middleFrontiers = doc.frontiers();
    list.move(3, 1);
    doc.commit();
    const latestFrontiers = doc.frontiers();
    const reset = vi.spyOn(list, "_reset");

    for (const frontiers of [
      baseFrontiers,
      middleFrontiers,
      latestFrontiers,
      baseFrontiers,
      latestFrontiers,
    ]) {
      const expected = doc.forkAt(frontiers).getMovableList("list").toJSON();
      doc.checkout(frontiers);
      expect(list.toJSON()).toEqual(expected);
    }
    expect(reset).not.toHaveBeenCalled();
  });

  test("switches directly between concurrent movable-list move branches", () => {
    const base = new LoroDoc();
    base.setPeerId(1);
    const baseList = base.getMovableList("list");
    for (const value of ["a", "b", "c", "d"]) baseList.push(value);
    base.commit();

    const left = base.fork();
    left.setPeerId(2);
    left.getMovableList("list").move(0, 3);
    left.commit();
    const leftFrontiers = left.frontiers();

    const right = base.fork();
    right.setPeerId(3);
    right.getMovableList("list").move(3, 0);
    right.commit();
    const rightFrontiers = right.frontiers();

    left.import(right.export({ mode: "update" }));
    const mergedFrontiers = left.frontiers();
    const list = left.getMovableList("list");
    const reset = vi.spyOn(list, "_reset");

    for (const frontiers of [
      leftFrontiers,
      rightFrontiers,
      mergedFrontiers,
      rightFrontiers,
      leftFrontiers,
      mergedFrontiers,
    ] as const) {
      const expected = left.forkAt(frontiers).getMovableList("list").toJSON();
      left.checkout(frontiers);
      expect(list.toJSON()).toEqual(expected);
    }
    expect(reset).not.toHaveBeenCalled();
  });

  test("switches concurrent movable-list branches with inserted and deleted items", () => {
    const base = new LoroDoc();
    base.setPeerId(1);
    const baseList = base.getMovableList("list");
    for (const value of ["a", "b", "c", "d"]) baseList.push(value);
    base.commit();

    const left = base.fork();
    left.setPeerId(2);
    const leftList = left.getMovableList("list");
    leftList.move(0, 3);
    leftList.insert(1, "L");
    left.commit();
    const leftFrontiers = left.frontiers();

    const right = base.fork();
    right.setPeerId(3);
    const rightList = right.getMovableList("list");
    rightList.move(3, 0);
    rightList.delete(2, 1);
    right.commit();
    const rightFrontiers = right.frontiers();

    left.import(right.export({ mode: "update", from: base.oplogVersion() }));
    const mergedFrontiers = left.frontiers();
    const list = left.getMovableList("list");
    const reset = vi.spyOn(list, "_reset");

    for (const frontiers of [
      leftFrontiers,
      rightFrontiers,
      mergedFrontiers,
      rightFrontiers,
      leftFrontiers,
      mergedFrontiers,
    ]) {
      const expected = left.forkAt(frontiers).getMovableList("list").toJSON();
      left.checkout(frontiers);
      expect(list.toJSON()).toEqual(expected);
    }
    expect(reset).not.toHaveBeenCalled();
  });

  test("matches rebuilt movable-list branches across randomized concurrent moves", () => {
    let random = 0x7a_19_42_c3;
    const nextRandom = (): number => {
      random ^= random << 13;
      random ^= random >>> 17;
      random ^= random << 5;
      return random >>> 0;
    };

    for (let scenario = 0; scenario < 24; scenario += 1) {
      const base = new LoroDoc();
      base.setPeerId(1);
      const baseList = base.getMovableList("list");
      for (let index = 0; index < 8; index += 1) baseList.push(index);
      base.commit();

      const left = base.fork();
      left.setPeerId(2);
      const leftList = left.getMovableList("list");
      for (let move = 0; move < 4; move += 1) {
        const from = nextRandom() % leftList.length;
        const to = (from + 1 + (nextRandom() % (leftList.length - 1))) % leftList.length;
        leftList.move(from, to);
        left.commit();
      }
      const leftFrontiers = left.frontiers();

      const right = base.fork();
      right.setPeerId(3);
      const rightList = right.getMovableList("list");
      for (let move = 0; move < 4; move += 1) {
        const from = nextRandom() % rightList.length;
        const to =
          (from + 1 + (nextRandom() % (rightList.length - 1))) % rightList.length;
        rightList.move(from, to);
        right.commit();
      }
      const rightFrontiers = right.frontiers();

      const importMirror = left.fork();
      let importBatch: LoroEventBatch | undefined;
      const unsubscribe = left.subscribe((event) => {
        importBatch = event;
      });
      left.import(right.export({ mode: "update", from: base.oplogVersion() }));
      unsubscribe();
      const mergedFrontiers = left.frontiers();
      const list = left.getMovableList("list");
      expect(importBatch).toBeDefined();
      importMirror.applyDiff(
        importBatch!.events.map(({ target, diff }) => [target, diff]),
      );
      expect(importMirror.getMovableList("list").toJSON()).toEqual(list.toJSON());
      expect(list.toJSON()).toEqual(
        left.forkAt(mergedFrontiers).getMovableList("list").toJSON(),
      );
      const reset = vi.spyOn(list, "_reset");
      for (const frontiers of [
        leftFrontiers,
        rightFrontiers,
        mergedFrontiers,
        leftFrontiers,
        mergedFrontiers,
        rightFrontiers,
      ]) {
        const expected = left.forkAt(frontiers).getMovableList("list").toJSON();
        left.checkout(frontiers);
        expect(list.toJSON()).toEqual(expected);
      }
      expect(reset).not.toHaveBeenCalled();

      const expectedRight = left.forkAt(rightFrontiers).getMovableList("list").toJSON();
      const mirror = left.forkAt(leftFrontiers);
      mirror.applyDiff(left.diff(leftFrontiers, rightFrontiers, false));
      expect(mirror.getMovableList("list").toJSON()).toEqual(expectedRight);
      expect(reset).not.toHaveBeenCalled();
    }
  });

  test("switches a large contiguous text deletion without rebuilding the text", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const text = doc.getText("text");
    const value = "x".repeat(16_384);
    text.insert(0, value);
    doc.commit();
    const beforeDelete = doc.frontiers();

    text.delete(0, value.length);
    doc.commit();
    expect(text.toString()).toBe("");

    doc.checkout(beforeDelete);
    expect(text.toString()).toBe(value);
    const mirror = doc.forkAt(beforeDelete);
    let batch: LoroEventBatch | undefined;
    const unsubscribe = doc.subscribe((event) => {
      batch = event;
    });
    doc.checkoutToLatest();
    unsubscribe();
    expect(text.toString()).toBe("");
    expect(batch).toBeDefined();
    expect(JSON.stringify(batch!.events).length).toBeLessThan(256);
    mirror.applyDiff(batch!.events.map(({ target, diff }) => [target, diff]));
    expect(mirror.getText("text").toString()).toBe("");
  });

  test("keeps overlapping concurrent range deletions during branch switches", () => {
    const base = new LoroDoc();
    base.setPeerId(1);
    base.getText("text").insert(0, "x".repeat(1_024));
    base.commit();
    const baseFrontiers = base.frontiers();

    const left = base.fork();
    left.setPeerId(2);
    left.getText("text").delete(0, 1_024);
    left.commit();
    const leftFrontiers = left.frontiers();

    const right = base.fork();
    right.setPeerId(3);
    right.getText("text").delete(200, 600);
    right.commit();
    const rightFrontiers = right.frontiers();

    left.import(right.export({ mode: "update", from: base.oplogVersion() }));
    left.checkout(rightFrontiers);
    expect(left.getText("text").length).toBe(424);
    left.checkout(baseFrontiers);
    expect(left.getText("text").length).toBe(1_024);
    left.checkout(leftFrontiers);
    expect(left.getText("text").length).toBe(0);
  });
});
