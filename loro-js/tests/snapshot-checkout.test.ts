import { describe, expect, test } from "vitest";

import { LoroDoc, LoroList, LoroMap, LoroText, type Frontiers } from "../src/index";

/**
 * A snapshot import keeps non-root containers encoded until they are first read.
 * Checkout must not hydrate such a container with its latest state after moving
 * the document to another version.
 */
function buildHistory(): { doc: LoroDoc; versions: Frontiers[] } {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  const root = doc.getMap("root");
  const map = root.setContainer("map", new LoroMap());
  const list = root.setContainer("list", new LoroList());
  const text = root.setContainer("text", new LoroText());
  map.set("a", 1);
  list.push(1);
  text.insert(0, "abc");
  doc.commit();
  const versions = [doc.frontiers()];
  map.set("a", 2);
  map.set("b", 3);
  list.push(2);
  text.insert(0, "X");
  doc.commit();
  versions.push(doc.frontiers());
  map.delete("b");
  list.delete(0, 1);
  text.delete(1, 2);
  doc.commit();
  versions.push(doc.frontiers());
  return { doc, versions };
}

const valuesAt = (doc: LoroDoc, versions: readonly Frontiers[]): unknown[] =>
  versions.map((version) => {
    doc.checkout(version);
    return doc.toJSON();
  });

describe("checkout after a lazy snapshot import", () => {
  test("matches the full-history document at every version", () => {
    const { doc, versions } = buildHistory();
    const expected = valuesAt(doc, versions);
    doc.checkoutToLatest();
    const bytes = doc.export({ mode: "snapshot" });

    // Checking out before anything is read exercises both the version transition
    // and the full-replay paths on unhydrated children.
    for (const order of [versions, [...versions].reverse()]) {
      const loaded = new LoroDoc();
      loaded.import(bytes);
      const expectedInOrder = order.map((version) => expected[versions.indexOf(version)]);
      expect(valuesAt(loaded, order)).toEqual(expectedInOrder);
      loaded.checkoutToLatest();
      expect(loaded.toJSON()).toEqual(doc.toJSON());
    }
  });

  test("reads a child first touched after checkout at its historical state", () => {
    const { doc, versions } = buildHistory();
    const loaded = new LoroDoc();
    loaded.import(doc.export({ mode: "snapshot" }));
    loaded.checkout(versions[0]!);
    const root = loaded.getMap("root");
    expect((root.get("map") as LoroMap).toJSON()).toEqual({ a: 1 });
    expect((root.get("list") as LoroList).toJSON()).toEqual([1]);
    expect((root.get("text") as LoroText).toString()).toBe("abc");
  });

  test("keeps exported snapshots consistent after checkout round trips", () => {
    const { doc, versions } = buildHistory();
    const loaded = new LoroDoc();
    loaded.import(doc.export({ mode: "snapshot" }));
    loaded.checkout(versions[0]!);
    loaded.checkoutToLatest();
    const again = new LoroDoc();
    again.import(loaded.export({ mode: "snapshot" }));
    expect(again.toJSON()).toEqual(doc.toJSON());
    again.checkout(versions[1]!);
    doc.checkout(versions[1]!);
    expect(again.toJSON()).toEqual(doc.toJSON());
  });

  test("checks out a shallow snapshot through its retained history", () => {
    const { doc, versions } = buildHistory();
    const loaded = new LoroDoc();
    loaded.import(doc.export({ mode: "shallow-snapshot", frontiers: versions[0]! }));
    const expected = valuesAt(doc, versions);
    expect(valuesAt(loaded, versions)).toEqual(expected);
  });
});
