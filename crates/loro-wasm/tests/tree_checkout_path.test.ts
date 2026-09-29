import { describe, expect, it } from "vitest";
import { LoroDoc } from "../bundler/index";

// Repros from the loro.js differential fuzz (loro-dev/loro#1141). See
// context/tree-checkout-window.md.

function docWithPeer(peer: bigint): LoroDoc {
  const doc = new LoroDoc();
  doc.setPeerId(peer);
  return doc;
}

function merged(docs: LoroDoc[]): LoroDoc {
  const doc = new LoroDoc();
  for (const d of docs) doc.import(d.export({ mode: "update" }));
  return doc;
}

describe("tree checkout path", () => {
  it("does not panic on a checkout sequence with a subscriber", () => {
    const p1 = docWithPeer(1n);
    p1.getList("l").insert(0, "x");
    const a = p1.getTree("tr").createNode();
    const b = p1.getTree("tr").createNode(a.id);
    p1.commit();

    const p3 = docWithPeer(3n);
    p3.import(
      p1.export({
        mode: "updates-in-range",
        spans: [{ id: { peer: "1", counter: 0 }, len: 1 }],
      }),
    );
    p3.getList("l").insert(0, "y");
    p3.commit();

    const p2 = docWithPeer(2n);
    p2.import(p3.export({ mode: "update" }));
    p2.getCounter("c").increment(1);
    p2.commit();

    p3.import(p2.export({ mode: "update" }));
    p3.getMap("m").set("k", 1);
    p3.commit();

    p2.import(p1.export({ mode: "update" }));
    p2.getTree("tr").getNodeByID(b.id)!.data.set("k", true);
    p2.commit();

    const doc = merged([p1, p2, p3]);
    let events = 0;
    doc.subscribe(() => {
      events += 1;
    });
    for (const version of [
      [{ peer: "3" as const, counter: 1 }],
      [{ peer: "2" as const, counter: 1 }],
    ]) {
      doc.checkout(version);
      const fresh = merged([p1, p2, p3]);
      fresh.checkout(version);
      expect(doc.toJSON()).toStrictEqual(fresh.toJSON());
    }
    expect(events).toBeGreaterThan(0);
  });

  it("removes a node when checking out a concurrent version without its creation", () => {
    const [p1, p2, p3] = [1n, 2n, 3n].map(docWithPeer);
    p1.getCounter("c").increment(1);
    p1.commit(); // 0@1
    p3.import(p1.export({ mode: "update" }));
    p3.getTree("tr").createNode();
    p3.commit(); // 0@3, deps 0@1
    p2.getList("l").insert(0, 1);
    p2.getList("l").delete(0, 1);
    p2.commit(); // 0@2..1@2
    p2.import(p1.export({ mode: "update" }));
    p2.getCounter("c").increment(1);
    p2.commit(); // 2@2, deps [0@1, 1@2]
    p1.import(p2.export({ mode: "update" }));
    p1.getMap("m").set("k", 1);
    p1.commit(); // 1@1, deps 2@2
    p2.import(p3.export({ mode: "update" }));
    p2.getText("t").insert(0, "x");
    p2.commit(); // 3@2, deps [2@2, 0@3]

    const target = [{ peer: "1" as const, counter: 1 }];
    const direct = merged([p1, p2, p3]);
    direct.checkout(target);
    expect(direct.getTree("tr").toJSON()).toStrictEqual([]);

    const doc = merged([p1, p2, p3]);
    doc.checkout([{ peer: "2", counter: 3 }]);
    expect(doc.getTree("tr").toJSON().length).toBe(1);
    doc.checkout(target);
    expect(doc.getTree("tr").toJSON()).toStrictEqual([]);
    expect(doc.toJSON()).toStrictEqual(direct.toJSON());
  });
});

describe("tree checkout between old versions", () => {
  it("matches a direct checkout after scrubbing back and forth", () => {
    const doc = docWithPeer(1n);
    const tree = doc.getTree("tree");
    const nodes = Array.from({ length: 30 }, () => tree.createNode().id);
    doc.commit();
    let seed = 7;
    const next = (n: number) => {
      seed = (seed * 1103515245 + 12345) % 2147483648;
      return seed % n;
    };
    for (let i = 0; i < 1200; i++) {
      try {
        tree.move(nodes[next(nodes.length)], nodes[next(nodes.length)]);
      } catch {
        // moving a node under its own descendant is rejected
      }
      if (i % 20 === 19) doc.commit();
    }
    doc.commit();

    // Rejected moves record no op, so pick versions from the last counter.
    const last = doc.frontiers()[0].counter;
    const versions = [0.2, 0.25, 0.7].map((f) => [
      { peer: "1" as const, counter: Math.floor(last * f) },
    ]);
    const expected = versions.map((v) => {
      const fresh = new LoroDoc();
      fresh.import(doc.export({ mode: "update" }));
      fresh.checkout(v);
      return fresh.getTree("tree").toJSON();
    });
    for (let round = 0; round < 3; round++) {
      for (const [i, v] of versions.entries()) {
        doc.checkout(v);
        expect(doc.getTree("tree").toJSON()).toStrictEqual(expected[i]);
      }
    }
  });
});
