import { describe, expect, test } from "vitest";

import { SequenceEventDiff } from "../src/runtime/event-diff";
import { LoroDoc } from "../src/runtime";
import type { Delta, LoroEventBatch } from "../src/runtime/types";

describe("incremental event diffs", () => {
  test("composes randomized text edits against a string model", () => {
    const original = "abcdefghijklmnopqrstuvwxyz";
    const diff = new SequenceEventDiff("text", original.length);
    let current = original;
    let random = 0x51_7c_c1_b7;
    const next = (): number => {
      random ^= random << 13;
      random ^= random >>> 17;
      random ^= random << 5;
      return random >>> 0;
    };

    for (let step = 0; step < 1_000; step += 1) {
      if (current.length === 0 || (next() & 1) === 0) {
        const position = next() % (current.length + 1);
        const value = String.fromCharCode(65 + (next() % 26));
        diff.insertText(position, value);
        current = current.slice(0, position) + value + current.slice(position);
      } else {
        const position = next() % current.length;
        const length = 1 + (next() % Math.min(4, current.length - position));
        diff.delete(position, length);
        current = current.slice(0, position) + current.slice(position + length);
      }
    }

    const result = diff.toDiff();
    expect(result.type).toBe("text");
    expect(applyTextDelta(original, result.diff as Delta<string>[])).toBe(current);
  });

  test("composes randomized list edits against an array model", () => {
    const original = Array.from({ length: 32 }, (_, index) => index);
    const diff = new SequenceEventDiff("list", original.length);
    let current: unknown[] = [...original];
    let random = 0x19_96_03_14;
    const next = (): number => {
      random ^= random << 13;
      random ^= random >>> 17;
      random ^= random << 5;
      return random >>> 0;
    };

    for (let step = 0; step < 1_000; step += 1) {
      if (current.length === 0 || (next() & 1) === 0) {
        const position = next() % (current.length + 1);
        const value = `v${step}`;
        diff.insertList(position, [value]);
        current.splice(position, 0, value);
      } else {
        const position = next() % current.length;
        const length = 1 + (next() % Math.min(4, current.length - position));
        diff.delete(position, length);
        current.splice(position, length);
      }
    }

    const result = diff.toDiff();
    expect(result.type).toBe("list");
    expect(applyListDelta(original, result.diff as Delta<unknown[]>[])).toEqual(current);
  });

  test("keeps text formatting as an attributed retain", () => {
    const diff = new SequenceEventDiff("text", 6);
    diff.formatText(1, 3, "bold", true);
    expect(diff.toDiff()).toEqual({
      type: "text",
      diff: [{ retain: 1 }, { retain: 3, attributes: { bold: true } }],
    });
  });

  test("emits operation-composed local diffs for every indexed container", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    const text = source.getText("text");
    const list = source.getList("list");
    const movable = source.getMovableList("movable");
    const map = source.getMap("map");
    const counter = source.getCounter("counter");
    text.insert(0, "abcdef");
    list.push("a");
    list.push("b");
    list.push("c");
    movable.push("a");
    movable.push("b");
    movable.push("c");
    map.set("value", 1);
    counter.increment(2);
    source.commit();

    const target = new LoroDoc();
    target.setPeerId(2);
    target.getText("text").insert(0, "abcdef");
    target.getList("list").push("a");
    target.getList("list").push("b");
    target.getList("list").push("c");
    target.getMovableList("movable").push("a");
    target.getMovableList("movable").push("b");
    target.getMovableList("movable").push("c");
    target.getMap("map").set("value", 1);
    target.getCounter("counter").increment(2);
    target.commit();

    let batch: LoroEventBatch | undefined;
    source.subscribe((event) => {
      batch = event;
    });
    text.delete(1, 2);
    text.insert(2, "XY");
    text.mark({ start: 1, end: 4 }, "bold", true);
    list.delete(1, 1);
    list.insert(1, "B");
    movable.move(0, 2);
    movable.set(1, "B");
    map.set("value", 2);
    map.set("transient", true);
    map.delete("transient");
    counter.increment(3);
    counter.decrement(1);
    source.commit();

    expect(batch).toBeDefined();
    target.applyDiff(
      batch!.events.map(({ target: id, diff: eventDiff }) => [id, eventDiff]),
    );
    expect(target.toJSON()).toEqual(source.toJSON());
    expect(target.getText("text").toDelta()).toEqual(text.toDelta());
    expect(batch!.events.find(({ target: id }) => id === map.id)?.diff).toEqual({
      type: "map",
      updated: { value: 2 },
    });
    expect(batch!.events.find(({ target: id }) => id === counter.id)?.diff).toEqual({
      type: "counter",
      increment: 2,
    });
  });

  test("records imported concurrent edits at their actual visible positions", () => {
    const base = new LoroDoc();
    base.setPeerId(1);
    base.getText("text").insert(0, "abcd");
    for (const value of ["a", "b", "c", "d"]) base.getList("list").push(value);
    for (const value of ["a", "b", "c"]) {
      base.getMovableList("movable").push(value);
    }
    base.getMap("map").set("value", 1);
    base.getCounter("counter").increment(1);
    base.commit();
    const baseVersion = base.oplogVersion();

    const left = base.fork();
    left.setPeerId(2);
    left.getText("text").insert(2, "L");
    left.getText("text").delete(0, 1);
    left.getText("text").mark({ start: 0, end: 3 }, "bold", true);
    left.getList("list").insert(2, "L");
    left.getList("list").delete(0, 1);
    left.getMovableList("movable").move(0, 2);
    left.getMovableList("movable").set(1, "B");
    left.getMap("map").set("value", 2);
    left.getCounter("counter").increment(2);
    left.commit();
    const update = left.export({ mode: "update", from: baseVersion });

    const right = base.fork();
    right.setPeerId(3);
    right.getText("text").insert(2, "R");
    right.getList("list").insert(2, "R");
    right.getMovableList("movable").insert(1, "R");
    right.commit();
    const mirror = right.fork();

    let batch: LoroEventBatch | undefined;
    right.subscribe((event) => {
      batch = event;
    });
    right.import(update);

    expect(batch).toBeDefined();
    mirror.applyDiff(
      batch!.events.map(({ target: id, diff: eventDiff }) => [id, eventDiff]),
    );
    expect(mirror.toJSON()).toEqual(right.toJSON());
    expect(mirror.getText("text").toDelta()).toEqual(right.getText("text").toDelta());
  });

  test("emits only directly changed tree nodes", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const tree = doc.getTree("tree");
    tree.createNode();
    tree.createNode();
    tree.createNode();
    doc.commit();
    let batch: LoroEventBatch | undefined;
    doc.subscribe((event) => {
      batch = event;
    });

    const created = tree.createNode(undefined, 0);
    doc.commit();
    expect(batch?.events[0]?.diff).toEqual({
      type: "tree",
      diff: [
        {
          target: created.id,
          action: "create",
          parent: undefined,
          index: 0,
          fractionalIndex: created.fractionalIndex(),
        },
      ],
    });

    const moved = tree.roots()[2]!;
    const oldIndex = moved.index();
    moved.move(undefined, 0);
    doc.commit();
    expect(batch?.events[0]?.diff).toEqual({
      type: "tree",
      diff: [
        {
          target: moved.id,
          action: "move",
          parent: undefined,
          index: 0,
          fractionalIndex: moved.fractionalIndex(),
          oldParent: undefined,
          oldIndex,
        },
      ],
    });
  });
});

function applyTextDelta(original: string, delta: readonly Delta<string>[]): string {
  let cursor = 0;
  let output = "";
  for (const operation of delta) {
    if ("retain" in operation) {
      output += original.slice(cursor, cursor + operation.retain);
      cursor += operation.retain;
    } else if ("delete" in operation) {
      cursor += operation.delete;
    } else {
      output += operation.insert;
    }
  }
  return output + original.slice(cursor);
}

function applyListDelta(
  original: readonly unknown[],
  delta: readonly Delta<unknown[]>[],
): unknown[] {
  let cursor = 0;
  const output: unknown[] = [];
  for (const operation of delta) {
    if ("retain" in operation) {
      output.push(...original.slice(cursor, cursor + operation.retain));
      cursor += operation.retain;
    } else if ("delete" in operation) {
      cursor += operation.delete;
    } else {
      output.push(...operation.insert);
    }
  }
  output.push(...original.slice(cursor));
  return output;
}
