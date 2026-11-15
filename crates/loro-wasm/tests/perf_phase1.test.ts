import { describe, expect, it } from "vitest";
import {
  LoroCounter,
  LoroDoc,
  LoroList,
  LoroMap,
  LoroMovableList,
  LoroText,
  LoroTree,
} from "../bundler/index";

describe("Phase 1 optimizations", () => {
  it("toJSON handles all containers, mergeable children, tree meta, binary, and hidden empty roots", () => {
    const doc = new LoroDoc();
    doc.setPeerId("1");

    const text = doc.getText("text");
    text.insert(0, "Hello");
    text.mark({ start: 0, end: 5 }, "bold", true);

    const list = doc.getList("list");
    list.push(42);
    list.push(null);
    list.push(true);
    list.push(new Uint8Array([1, 2, 3]));
    const subMap = list.insertContainer(4, new LoroMap());
    subMap.set("nested", "val");

    const mlist = doc.getMovableList("mlist");
    mlist.push("a");
    mlist.push("b");
    mlist.move(0, 1);

    const map = doc.getMap("map");
    map.set("plainObj", { arr: [1, { deep: "ok" }] });
    map.ensureMergeableMap("mMap").set("k", "v");
    map.ensureMergeableList("mList").push("item");
    map.ensureMergeableText("mText").insert(0, "mergeable");
    map.ensureMergeableCounter("mCounter").increment(7);

    const counter = doc.getCounter("counter");
    counter.increment(12.5);

    const tree = doc.getTree("tree");
    const rootNode = tree.createNode();
    rootNode.data.set("title", "root");
    const childNode = rootNode.createNode();
    childNode.data.set("title", "child");

    doc.getMap("emptyHidden");
    doc.commit();

    const fullJson = doc.toJSON();
    expect(fullJson.text).toBe("Hello");
    expect(fullJson.list[0]).toBe(42);
    expect(fullJson.list[1]).toBeNull();
    expect(fullJson.list[2]).toBe(true);
    expect(Array.from(fullJson.list[3])).toEqual([1, 2, 3]);
    expect(fullJson.list[4]).toEqual({ nested: "val" });
    expect(fullJson.mlist).toEqual(["b", "a"]);
    expect(fullJson.map).toEqual({
      plainObj: { arr: [1, { deep: "ok" }] },
      mMap: { k: "v" },
      mList: ["item"],
      mText: "mergeable",
      mCounter: 7,
    });
    expect(fullJson.counter).toBe(12.5);
    expect(fullJson.emptyHidden).toEqual({});
    expect(fullJson.tree).toHaveLength(1);
    expect(fullJson.tree[0].meta).toEqual({ title: "root" });
    expect(typeof fullJson.tree[0].fractional_index).toBe("string");
    expect(fullJson.tree[0].children).toHaveLength(1);
    expect(fullJson.tree[0].children[0].meta).toEqual({ title: "child" });

    doc.setHideEmptyRootContainers(true);
    const hiddenJson = doc.toJSON();
    expect("emptyHidden" in hiddenJson).toBe(false);

    // Imported snapshot must produce the identical JSON structure.
    const snap = doc.export({ mode: "snapshot" });
    const restored = LoroDoc.fromSnapshot(snap);
    restored.setHideEmptyRootContainers(true);
    expect(restored.toJSON()).toEqual(hiddenJson);
  });

  it("toJSON handles deeply nested maps and lists without stack overflow", () => {
    const doc = new LoroDoc();
    let cur = doc.getMap("deep");
    for (let i = 0; i < 600; i++) {
      cur = cur.setContainer("next", new LoroMap());
    }
    const leaf = cur.setContainer("leaf", new LoroList());
    leaf.push("bottom");
    doc.commit();

    let node: any = doc.toJSON().deep;
    for (let i = 0; i < 600; i++) {
      node = node.next;
    }
    expect(node.leaf).toEqual(["bottom"]);
  });

  it("streams large (>256 KiB) importBatch and importBatchDetailed accurately", () => {
    const makeIncompressibleText = (seed: number, len: number): string => {
      const alphabet =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
      let state = seed >>> 0;
      const chars: string[] = new Array(len);
      for (let i = 0; i < len; i++) {
        state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
        chars[i] = alphabet[(state >>> 26) & 63];
      }
      return chars.join("");
    };

    const src = new LoroDoc();
    src.setPeerId("1");
    const chunkA = makeIncompressibleText(1, 140 * 1024);
    const chunkB = makeIncompressibleText(2, 140 * 1024);
    const chunkC = makeIncompressibleText(3, 140 * 1024);

    src.getText("t").insert(0, chunkA);
    src.commit();
    const v1 = src.version();
    const snap = src.export({ mode: "snapshot" });

    src.getText("t").insert(chunkA.length, chunkB);
    src.commit();
    const v2 = src.version();
    const u1 = src.export({ mode: "update", from: v1 });

    src.getText("t").insert(chunkA.length + chunkB.length, chunkC);
    src.commit();
    const u2 = src.export({ mode: "update", from: v2 });

    expect(snap.byteLength + u1.byteLength + u2.byteLength).toBeGreaterThan(
      256 * 1024,
    );

    const d1 = new LoroDoc();
    const status = d1.importBatch([u2, u1, snap]);
    expect(status.pending).toBeNull();
    expect(d1.getText("t").toString().length).toBe(
      chunkA.length + chunkB.length + chunkC.length,
    );

    const d2 = new LoroDoc();
    const bad = new Uint8Array([1, 2, 3, 4]);
    const detail = d2.importBatchDetailed([u2, bad, u1, snap]);
    expect(detail.rollbackError).toBeNull();
    expect(detail.blobs.map((b) => b.type)).toEqual([
      "imported",
      "failed",
      "imported",
      "imported",
    ]);
    expect(d2.getText("t").toString()).toBe(d1.getText("t").toString());
  });

  it("compactChangeStore flushes pending subscription events and free helpers preserve state", async () => {
    const doc = new LoroDoc();
    doc.setPeerId("1");
    let eventCount = 0;
    const errors: unknown[] = [];
    const origError = console.error;
    console.error = (...args: unknown[]) => {
      errors.push(args);
    };

    try {
      doc.subscribe(() => {
        eventCount += 1;
      });
      doc.getText("t").insert(0, "one");
      doc.compactChangeStore();
      const v1 = doc.frontiers();

      doc.getText("t").insert(3, " two");
      doc.commit();
      doc.checkout(v1);
      expect(doc.getText("t").toString()).toBe("one");

      doc.freeHistoryCache();
      doc.freeDiffCalculator();
      doc.checkoutToLatest();
      expect(doc.getText("t").toString()).toBe("one two");

      await new Promise((resolve) => setTimeout(resolve, 10));
      expect(eventCount).toBeGreaterThanOrEqual(2);
      expect(errors).toEqual([]);
    } finally {
      console.error = origError;
    }
  });
});
