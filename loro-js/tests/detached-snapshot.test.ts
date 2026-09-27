import { describe, expect, test } from "vitest";

import { LoroDoc, LoroText } from "../src/index";

function buildHistory() {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  const map = doc.getMap("map");
  map.set("a", 1);
  map.setContainer("text", new LoroText()).insert(0, "hello");
  doc.getList("list").push(1);
  doc.commit();
  const first = doc.frontiers();
  map.set("a", 2);
  (map.get("text") as LoroText).insert(5, " world");
  (map.get("text") as LoroText).delete(0, 1);
  doc.getList("list").push(2);
  doc.commit();
  return { doc, first, latest: doc.frontiers(), latestValue: doc.toJSON() };
}

describe("snapshot export from a detached document", () => {
  test("encodes the latest state and keeps the checkout", () => {
    const { doc, first, latest, latestValue } = buildHistory();
    doc.checkout(first);
    const firstValue = doc.toJSON();
    const text = doc.getMap("map").get("text") as LoroText;

    const bytes = doc.export({ mode: "snapshot" });
    expect(doc.isDetached()).toBe(true);
    expect(doc.frontiers()).toEqual(first);
    expect(doc.toJSON()).toEqual(firstValue);
    expect(text.toString()).toBe("hello");
    doc.checkoutToLatest();
    expect(doc.toJSON()).toEqual(latestValue);
    doc.checkout(first);

    const loaded = new LoroDoc();
    loaded.import(bytes);
    expect(loaded.toJSON()).toEqual(latestValue);
    expect(loaded.frontiers()).toEqual(latest);
    expect(loaded.oplogFrontiers()).toEqual(latest);
    // Retreating snapshot-imported state over a delete is covered by the
    // snapshot-checkout tests; here, check that history reaches the latest.
    loaded.checkout(latest);
    expect(loaded.toJSON()).toEqual(latestValue);
  });

  test("includes updates imported while detached into a lazily imported document", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    source.getMap("m").set("x", 1);
    source.commit();
    const version = source.oplogVersion();
    const target = new LoroDoc();
    target.import(source.export({ mode: "snapshot" }));
    target.detach();
    source.getMap("m").set("x", 2);
    source.commit();
    target.import(source.export({ mode: "update", from: version }));

    const loaded = new LoroDoc();
    loaded.import(target.export({ mode: "snapshot" }));
    expect(loaded.toJSON()).toEqual({ m: { x: 2 } });
    expect(loaded.oplogFrontiers()).toEqual(source.frontiers());
    loaded.checkout(source.frontiers());
    expect(loaded.toJSON()).toEqual({ m: { x: 2 } });
    // The exporting document keeps its detached state.
    expect(target.isDetached()).toBe(true);
    expect(target.toJSON()).toEqual({ m: { x: 1 } });
  });

  test("includes updates imported while detached", () => {
    const { doc, first } = buildHistory();
    const other = new LoroDoc();
    other.setPeerId(2);
    other.import(doc.export({ mode: "update" }));
    other.getMap("map").set("b", 3);
    other.commit();

    doc.checkout(first);
    doc.import(other.export({ mode: "update", from: doc.oplogVersion() }));
    const loaded = new LoroDoc();
    loaded.import(doc.export({ mode: "snapshot" }));
    expect(loaded.toJSON()).toEqual(other.toJSON());
    expect(loaded.oplogFrontiers()).toEqual(other.oplogFrontiers());
  });
});
