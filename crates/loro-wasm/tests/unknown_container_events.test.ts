import { describe, expect, it } from "vitest";
import { LoroCounter, LoroDoc, LoroEventBatch, LoroMap } from "../bundler/index";

// Events that touch a container of a type unknown to this version (created by
// a newer loro-crdt). #1142 stopped one such event from dropping the whole
// batch. A map event that sets a key to an unknown container used to be left
// out as a whole, losing the known keys it also updated. See loro-dev/loro#1151.

/** Rewrites every Counter in `src`'s JSON updates into an unknown type. */
function asNewerVersion(src: LoroDoc) {
  return JSON.parse(
    JSON.stringify(src.exportJsonUpdates())
      .replace(/(cid:\d+@\d+):Counter/g, "$1:Unknown(9)")
      .split('"type":"counter"')
      .join('"type":"unknown"'),
  );
}

describe("events next to unknown containers", () => {
  it("delivers the known keys of a map event that also sets an unknown child", async () => {
    const src = new LoroDoc();
    src.setPeerId(1);
    const m = src.getMap("m");
    m.set("title", "x");
    m.setContainer("u", new LoroCounter()).increment(1);
    m.set("n", 2);
    src.commit();

    const doc = new LoroDoc();
    const batches: LoroEventBatch[] = [];
    doc.subscribe((e) => batches.push(e));
    const containerEvents: LoroEventBatch[] = [];
    doc.getMap("m").subscribe((e) => containerEvents.push(e));
    doc.importJsonUpdates(asNewerVersion(src));
    await Promise.resolve();

    for (const batch of [batches, containerEvents]) {
      expect(batch.length).toBe(1);
      const mapEvent = batch[0].events.find((e) => e.target === "cid:root-m:Map");
      expect(mapEvent?.diff).toStrictEqual({
        type: "map",
        // The unknown child `u` has no JS value, so its entry is left out.
        updated: { title: "x", n: 2 },
      });
    }
    expect(doc.toJSON()).toStrictEqual({ m: { title: "x", u: null, n: 2 } });
  });

  it("a nested map event with an unknown child keeps its known keys", async () => {
    const src = new LoroDoc();
    src.setPeerId(1);
    const inner = src.getMap("m").setContainer("inner", new LoroMap());
    inner.set("k", 1);
    inner.setContainer("u", new LoroCounter()).increment(1);
    src.commit();

    const doc = new LoroDoc();
    const batches: LoroEventBatch[] = [];
    doc.subscribe((e) => batches.push(e));
    doc.importJsonUpdates(asNewerVersion(src));
    await Promise.resolve();

    const innerEvent = batches[0].events.find((e) => e.path.length === 2);
    expect(innerEvent?.diff).toStrictEqual({ type: "map", updated: { k: 1 } });
  });
});
