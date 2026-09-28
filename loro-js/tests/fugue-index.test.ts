import { describe, expect, test, vi } from "vitest";

import { LoroDoc, LoroText } from "../src/index";
import { SequenceIndex } from "../src/runtime/sequence-index";

describe("Fugue origin index", () => {
  test("does not probe every descendant in a concurrent text run", () => {
    const probeCount = (length: number): number => {
      const text = new LoroText();
      text._insertFugue(0, "a".repeat(length), { peer: 1n, counter: 0 }, 0, new Map());

      const atPhysical = vi.spyOn(SequenceIndex.prototype, "atPhysicalRaw");
      text._insertFugue(0, "b", { peer: 2n, counter: 0 }, 0, new Map());
      const probes = atPhysical.mock.calls.length;
      atPhysical.mockRestore();

      expect(text.toString()).toBe(`${"a".repeat(length)}b`);
      return probes;
    };

    const shortRunProbes = probeCount(128);
    const longRunProbes = probeCount(16_384);
    expect(longRunProbes).toBeLessThanOrEqual(shortRunProbes + 2);
    expect(longRunProbes).toBeLessThan(8);
  });

  test("finds the end of a concurrent run under the origin without probing it", () => {
    const probeCount = (length: number): number => {
      const text = new LoroText();
      text._insertFugue(0, "x", { peer: 1n, counter: 0 }, 0, new Map());
      // A concurrent root: it sorts after x and stays after x's subtree.
      text._insertFugue(0, "n", { peer: 4n, counter: 0 }, 0, new Map());
      text._insertFugue(
        1,
        "a".repeat(length),
        { peer: 1n, counter: 1 },
        1,
        new Map([[1n, 1]]),
      );
      expect(text.toString()).toBe(`x${"a".repeat(length)}n`);

      const atPhysical = vi.spyOn(SequenceIndex.prototype, "atPhysicalRaw");
      text._insertFugue(1, "b", { peer: 2n, counter: 0 }, 1, new Map([[1n, 1]]));
      const probes = atPhysical.mock.calls.length;
      atPhysical.mockRestore();

      expect(text.toString()).toBe(`x${"a".repeat(length)}bn`);
      return probes;
    };

    const shortRunProbes = probeCount(128);
    const longRunProbes = probeCount(16_384);
    // A binary search over the interval, not a scan of the run.
    expect(longRunProbes).toBeLessThanOrEqual(shortRunProbes + 16);
    expect(longRunProbes).toBeLessThan(64);
  });

  // An insert concurrent with a run under its origin must stop at the first
  // concurrent element whose origin is further left, as Rust's scan does.
  // The indexed path used to jump to the origin-right element instead.
  test.each(["list", "text"] as const)(
    "orders a concurrent %s insert before an element from an older origin",
    (kind) => {
      const peers = [1, 2, 3, 4].map((peer) => {
        const doc = new LoroDoc();
        doc.setPeerId(peer);
        return doc;
      });
      const [p1, p2, p3, p4] = peers as [LoroDoc, LoroDoc, LoroDoc, LoroDoc];
      const insert = (doc: LoroDoc, pos: number, value: string): void => {
        if (kind === "list") doc.getList("seq").insert(pos, value);
        else doc.getText("seq").insert(pos, value);
        doc.commit();
      };
      const read = (doc: LoroDoc): string =>
        kind === "list"
          ? (doc.getList("seq").toArray() as string[]).join("")
          : doc.getText("seq").toString();

      insert(p1, 0, "A");
      insert(p2, 0, "B");
      insert(p4, 0, "N");
      p3.import(p1.export({ mode: "update" }));
      p3.import(p4.export({ mode: "update" }));
      expect(read(p3)).toBe("AN");
      // Y: origin left A, origin right N.
      insert(p3, 1, "Y");
      p1.import(p2.export({ mode: "update" }));
      p1.import(p4.export({ mode: "update" }));
      expect(read(p1)).toBe("ABN");
      // X: origin left A, origin right B.
      insert(p1, 1, "X");
      p1.import(p3.export({ mode: "update" }));

      const replay = new LoroDoc();
      for (const peer of [p4, p3, p2, p1]) replay.import(peer.export({ mode: "update" }));
      // loro-crdt gives AXYBN for every import order.
      expect(read(replay)).toBe("AXYBN");
      expect(read(p1)).toBe("AXYBN");
    },
  );

  test("places an insert concurrent with a run before a concurrent root, as Rust does", () => {
    const [p1, p2, p4] = [1, 2, 4].map((peer) => {
      const doc = new LoroDoc();
      doc.setPeerId(peer);
      return doc;
    }) as [LoroDoc, LoroDoc, LoroDoc];
    p1.getText("text").insert(0, "x");
    p1.commit();
    p2.import(p1.export({ mode: "update" }));
    p1.getText("text").insert(1, "aaaa");
    p1.commit();
    p4.getText("text").insert(0, "n");
    p4.commit();
    p2.getText("text").insert(1, "b");
    p2.commit();

    for (const order of [
      [p1, p4, p2],
      [p2, p4, p1],
      [p4, p1, p2],
    ]) {
      const doc = new LoroDoc();
      for (const peer of order) doc.import(peer.export({ mode: "update" }));
      // loro-crdt gives xaaaabn; the indexed path used to give xaaaanb.
      expect(doc.getText("text").toString()).toBe("xaaaabn");
    }
  });
});
