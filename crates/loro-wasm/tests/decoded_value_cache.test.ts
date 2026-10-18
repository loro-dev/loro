import { describe, expect, it } from "vitest";
import { LoroDoc, LoroMap } from "../bundler";

describe("decoded value cache limit", () => {
  it("keeps the default and copies local settings independently into forks", () => {
    const doc = new LoroDoc();
    expect(doc.getDecodedValueCacheLimit()).toBe(2048);
    doc.getMap("map").set("key", "value");
    doc.commit();
    doc.setDecodedValueCacheLimit(8192);
    const fork = doc.fork();
    const forkAt = doc.forkAt(doc.frontiers());
    expect(fork.getDecodedValueCacheLimit()).toBe(8192);
    expect(forkAt.getDecodedValueCacheLimit()).toBe(8192);
    doc.setDecodedValueCacheLimit(0);
    forkAt.setDecodedValueCacheLimit(1);
    expect(fork.getDecodedValueCacheLimit()).toBe(8192);
    const restored = LoroDoc.fromSnapshot(doc.export({ mode: "snapshot" }));
    expect(restored.getDecodedValueCacheLimit()).toBe(2048);
    expect(restored.toJSON()).toEqual(doc.toJSON());
    for (const d of [doc, fork, forkAt, restored]) d.free();
  });

  it.each([
    -1,
    0.5,
    NaN,
    Infinity,
    -Infinity,
    2 ** 32,
    Number.MAX_SAFE_INTEGER,
  ])(
    "rejects invalid limit %s without changing the setting or trapping wasm",
    (limit) => {
      const doc = new LoroDoc();
      expect(() => doc.setDecodedValueCacheLimit(limit)).toThrow(
        /finite integer/,
      );
      expect(doc.getDecodedValueCacheLimit()).toBe(2048);
      doc.getMap("map").set("still", "usable");
      expect(doc.getMap("map").get("still")).toBe("usable");
      doc.free();
    },
  );

  it("reads and edits snapshots after growing, shrinking, and disabling the cache", () => {
    const source = new LoroDoc();
    const list = source.getList("list");
    for (let i = 0; i < 32; i++) {
      list.pushContainer(new LoroMap()).set("key", i);
    }
    const doc = LoroDoc.fromSnapshot(source.export({ mode: "snapshot" }));
    for (const limit of [2 ** 32 - 1, 64, 1, 0]) {
      doc.setDecodedValueCacheLimit(limit);
      expect(doc.getDecodedValueCacheLimit()).toBe(limit);
      for (let pass = 0; pass < 2; pass++) {
        for (let i = 0; i < 32; i++) {
          expect((doc.getList("list").get(i) as LoroMap).get("key")).toBe(i);
        }
      }
    }
    (doc.getList("list").get(0) as LoroMap).set("edited", true);
    doc.setDecodedValueCacheLimit(0);
    const restored = LoroDoc.fromSnapshot(doc.export({ mode: "snapshot" }));
    expect(restored.toJSON()).toEqual(doc.toJSON());
    for (const d of [source, doc, restored]) d.free();
  });
});
