import { describe, expect, test } from "vitest";

import { LoroDoc, LoroList, LoroText } from "../src/index";
import type { ContainerID, Delta, LoroEventBatch } from "../src/index";

// A child container that an event attaches starts from empty for a listener,
// so, as in Rust's DocState::apply_diff, its event must carry its whole state
// ("revival") rather than a delta against the state it had while hidden.

function peers(count: number): LoroDoc[] {
  return Array.from({ length: count }, (_, index) => {
    const doc = new LoroDoc();
    doc.setPeerId(index + 1);
    return doc;
  });
}

function merged(...docs: LoroDoc[]): LoroDoc {
  const doc = new LoroDoc();
  for (const source of docs) doc.import(source.export({ mode: "update" }));
  return doc;
}

function eventsOf(batches: readonly LoroEventBatch[]): unknown[] {
  return batches.flatMap((batch) =>
    batch.events.map((event) => [
      event.target,
      JSON.parse(JSON.stringify(event.diff)) as unknown,
    ]),
  );
}

describe("events for child containers attached by a checkout", () => {
  // The expected events are exactly what loro-crdt emits.
  test("carry the full state of a map child set back from null", () => {
    const [p1] = peers(1) as [LoroDoc];
    const text = p1.getMap("map").setContainer("key", new LoroText());
    text.insert(0, "😀");
    p1.getMap("map").set("key", null);
    p1.commit();
    const doc = merged(p1);
    const batches: LoroEventBatch[] = [];
    doc.subscribe((batch) => batches.push(batch));
    doc.checkout([{ peer: "1", counter: 1 }]);
    expect(doc.toJSON()).toEqual({ map: { key: "😀" } });
    expect(eventsOf(batches)).toEqual([
      ["cid:root-map:Map", { type: "map", updated: { key: "😀" } }],
      [text.id, { type: "text", diff: [{ insert: "😀" }] }],
    ]);
  });

  test("carry the full state of a list child whose element is restored", () => {
    const [p1] = peers(1) as [LoroDoc];
    const child = p1.getList("list").insertContainer(0, new LoroList());
    p1.commit();
    child.insert(0, 5);
    p1.commit();
    p1.getList("list").delete(0, 1);
    p1.commit();
    const doc = merged(p1);
    const batches: LoroEventBatch[] = [];
    doc.subscribe((batch) => batches.push(batch));
    doc.checkout([{ peer: "1", counter: 1 }]);
    expect(doc.toJSON()).toEqual({ list: [[5]] });
    expect(eventsOf(batches)).toEqual([
      ["cid:root-list:List", { type: "list", diff: [{ insert: [[5]] }] }],
      [child.id, { type: "list", diff: [{ insert: [5] }] }],
    ]);
  });

  test("replace a delta against the hidden state with the full state", () => {
    const [p1, p2] = peers(2) as [LoroDoc, LoroDoc];
    const text = p1.getMap("map").setContainer("key", new LoroText());
    p1.commit();
    p2.import(p1.export({ mode: "update" }));
    // Edited concurrently with the key being overwritten.
    (p2.getContainerById(text.id) as LoroText).insert(0, "ab");
    p2.commit();
    p1.getMap("map").set("key", "scalar");
    p1.commit();
    const doc = merged(p1, p2);
    expect(doc.toJSON()).toEqual({ map: { key: "scalar" } });
    const batches: LoroEventBatch[] = [];
    doc.subscribe((batch) => batches.push(batch));
    doc.checkout([
      { peer: "1", counter: 0 },
      { peer: "2", counter: 1 },
    ]);
    expect(doc.toJSON()).toEqual({ map: { key: "ab" } });
    // Before, the text event deleted "ab" from a child the listener had just
    // created empty.
    expect(eventsOf(batches)).toEqual([
      ["cid:root-map:Map", { type: "map", updated: { key: "ab" } }],
      [text.id, { type: "text", diff: [{ insert: "ab" }] }],
    ]);
  });

  test("revive a list child that a whole-value list diff deletes and re-inserts", () => {
    const [p1, p2, p3] = peers(3) as [LoroDoc, LoroDoc, LoroDoc];
    p1.getList("list").insert(0, "s");
    p1.commit();
    p2.import(p1.export({ mode: "update" }));
    const text = p2.getList("list").insertContainer(1, new LoroText());
    text.insert(0, "a");
    p2.commit();
    p3.import(p1.export({ mode: "update" }));
    p3.import(p2.export({ mode: "update" }));
    const version = p3.frontiers();
    p1.getList("list").insert(1, true);
    p1.commit();
    p3.import(p1.export({ mode: "update" }));
    p3.checkout(version);
    // Imported while detached, so checkoutToLatest replays and diffs values.
    p2.getList("list").push(false);
    p2.commit();
    p3.import(p2.export({ mode: "update", from: p3.oplogVersion() }));

    const batches: LoroEventBatch[] = [];
    p3.subscribe((batch) => batches.push(batch));
    const model = new Map<ContainerID, unknown[] | string>([
      ["cid:root-list:List", ["s", { child: text.id }]],
      [text.id, "a"],
    ]);
    p3.checkoutToLatest();
    // Apply the events the way a listener does: an inserted child restarts
    // from empty.
    for (const event of batches.flatMap((batch) => batch.events)) {
      if (event.diff.type === "text") {
        model.set(
          event.target,
          applyText(model.get(event.target) as string, event.diff.diff),
        );
      } else if (event.diff.type === "list") {
        const next: unknown[] = [];
        const current = (model.get(event.target) ?? []) as unknown[];
        let index = 0;
        for (const item of event.diff.diff) {
          if ("retain" in item) {
            next.push(...current.slice(index, index + item.retain));
            index += item.retain;
          } else if ("delete" in item) {
            index += item.delete;
          } else {
            for (const value of item.insert) {
              if (value instanceof LoroText) {
                model.set(value.id, "");
                next.push({ child: value.id });
              } else {
                next.push(value);
              }
            }
          }
        }
        next.push(...current.slice(index));
        model.set(event.target, next);
      }
    }
    const list = model.get("cid:root-list:List") as unknown[];
    const resolved = list.map((value) =>
      typeof value === "object" && value !== null && "child" in value
        ? model.get((value as { child: ContainerID }).child)
        : value,
    );
    expect(p3.toJSON()).toEqual({ list: ["s", true, "a", false] });
    expect(resolved).toEqual(["s", true, "a", false]);
  });
});

function applyText(text: string, delta: readonly Delta<string>[]): string {
  let output = "";
  let cursor = 0;
  for (const item of delta) {
    if ("retain" in item) {
      output += text.slice(cursor, cursor + item.retain);
      cursor += item.retain;
    } else if ("delete" in item) {
      cursor += item.delete;
    } else {
      output += item.insert;
    }
  }
  return output + text.slice(cursor);
}
