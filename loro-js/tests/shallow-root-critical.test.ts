import { describe, expect, test } from "vitest";

import { LoroDoc } from "../src/index";
import type { Frontiers } from "../src/index";

// Ports of crates/loro/tests/shallow_root_critical.rs. A shallow snapshot keeps
// every op from its root to the latest version, so the root must be a critical
// version: every retained op is causally after it, never concurrent with it
// (loro-dev/loro#1095). The roots below are the ones loro-crdt picks.

function shallowOf(doc: LoroDoc, frontiers: Frontiers): LoroDoc {
  const shallow = new LoroDoc();
  shallow.import(doc.export({ mode: "shallow-snapshot", frontiers }));
  return shallow;
}

/** Every op of `doc` is in the root's causal past or has the root in its own. */
function expectCritical(doc: LoroDoc, root: Frontiers): void {
  if (root.length === 0) return;
  const rootVersion = doc.frontiersToVV(root);
  for (const [peer, end] of doc.oplogVersion().toJSON()) {
    for (let counter = rootVersion.get(peer) ?? 0; counter < end; counter += 1) {
      const opVersion = doc.frontiersToVV([{ peer, counter }]);
      const order = rootVersion.compare(opVersion);
      if (order !== -1 && order !== 0) {
        throw new Error(`${counter}@${peer} is concurrent with the root`);
      }
    }
  }
}

describe("shallow snapshot root", () => {
  test("keeps the full history for independent heads", () => {
    for (let peers = 2; peers <= 4; peers += 1) {
      const doc = new LoroDoc();
      doc.setPeerId(1);
      doc.getText("t").insert(0, "a");
      doc.commit();
      for (let peer = 2; peer <= peers; peer += 1) {
        const other = new LoroDoc();
        other.setPeerId(peer);
        other.getText("t").insert(0, "b");
        other.commit();
        doc.import(other.export({ mode: "update" }));
      }
      const shallow = shallowOf(doc, doc.oplogFrontiers());
      expect(shallow.shallowSinceFrontiers()).toEqual([]);
      expect(shallow.toJSON()).toEqual(doc.toJSON());
    }
  });

  test("still trims to a root shared by every head", () => {
    const base = new LoroDoc();
    base.setPeerId(100);
    base.getText("t").insert(0, "root");
    base.commit();
    const snapshot = base.export({ mode: "snapshot" });
    for (let peer = 1; peer <= 5; peer += 1) {
      const fork = new LoroDoc();
      fork.setPeerId(peer);
      fork.import(snapshot);
      fork.getText("t").insert(0, "x");
      fork.commit();
      base.import(fork.export({ mode: "update" }));
    }
    const shallow = shallowOf(base, base.oplogFrontiers());
    expect(shallow.shallowSinceFrontiers()).toEqual([{ peer: "100", counter: 3 }]);
    expect(shallow.toJSON()).toEqual(base.toJSON());
  });

  test("moves below a past version when a later branch forked below it", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    doc.getText("t").insert(0, "0");
    doc.commit();
    const atA0 = doc.export({ mode: "snapshot" });
    doc.getText("t").insert(0, "1");
    doc.commit();
    doc.getText("t").insert(0, "2");
    doc.commit();
    const target = doc.oplogFrontiers();
    const fork = new LoroDoc();
    fork.setPeerId(2);
    fork.import(atA0);
    fork.getText("t").insert(1, "B");
    fork.commit();
    doc.import(fork.export({ mode: "update" }));

    const shallow = shallowOf(doc, target);
    // The requested version 2@1 is concurrent with 0@2; loro-crdt uses 0@1.
    expect(shallow.shallowSinceFrontiers()).toEqual([{ peer: "1", counter: 0 }]);
    expectCritical(doc, shallow.shallowSinceFrontiers());
    shallow.checkout(target);
    expect(shallow.getText("t").toString()).toBe("210");
    shallow.checkoutToLatest();
    expect(shallow.getText("t").toString()).toBe("210B");
  });

  test("keeps concurrent list order through checkouts of the retained range", () => {
    const p2 = new LoroDoc();
    p2.setPeerId(2);
    const p4 = new LoroDoc();
    p4.setPeerId(4);
    p4.getList("l").insert(0, false);
    p4.commit();
    p2.getList("l").insert(0, 45);
    p2.commit();
    p2.getMap("m").set("k", 1);
    p2.commit();
    p2.getMap("m").set("k", 2);
    p2.commit();
    p2.import(p4.export({ mode: "update" }));
    expect(p2.getList("l").toArray()).toEqual([45, false]);

    // 4@0 is concurrent with 2@1, so the root cannot be 2@1.
    const shallow = shallowOf(p2, [{ peer: "2", counter: 1 }]);
    expect(shallow.shallowSinceFrontiers()).toEqual([]);
    for (const frontiers of [
      [
        { peer: "4", counter: 0 },
        { peer: "2", counter: 2 },
      ],
      [{ peer: "2", counter: 2 }],
      [{ peer: "2", counter: 1 }],
    ] as Frontiers[]) {
      shallow.checkout(frontiers);
      const full = new LoroDoc();
      full.import(p2.export({ mode: "update" }));
      full.checkout(frontiers);
      expect(shallow.toJSON()).toEqual(full.toJSON());
    }
    shallow.checkoutToLatest();
    // Before the fix the retained concurrent insert came back as [false, 45].
    expect(shallow.getList("l").toArray()).toEqual([45, false]);
  });

  test("is critical on random multi-peer histories", () => {
    let random = 0x2c1b_3c6d;
    const next = (limit: number): number => {
      random ^= random << 13;
      random ^= random >>> 17;
      random ^= random << 5;
      return (random >>> 0) % limit;
    };
    for (let round = 0; round < 40; round += 1) {
      const docs = [1, 2, 3].map((peer) => {
        const doc = new LoroDoc();
        doc.setPeerId(peer);
        return doc;
      });
      const targets: Frontiers[] = [];
      for (let step = 0; step < 24; step += 1) {
        const doc = docs[next(docs.length)]!;
        if (next(4) === 0) {
          const other = docs[next(docs.length)]!;
          if (other !== doc) doc.import(other.export({ mode: "update" }));
        } else {
          const text = doc.getText("t");
          text.insert(next(text.length + 1), String.fromCharCode(97 + next(26)));
          doc.commit();
          if (next(3) === 0) targets.push(doc.oplogFrontiers());
        }
      }
      const doc = docs[0]!;
      for (const other of docs.slice(1)) doc.import(other.export({ mode: "update" }));
      for (const target of targets) {
        const shallow = shallowOf(doc, target);
        const root = shallow.shallowSinceFrontiers();
        expectCritical(doc, root);
        expect(shallow.toJSON()).toEqual(doc.toJSON());
        const rootVersion = doc.frontiersToVV(root);
        for (const version of targets) {
          const order = rootVersion.compare(doc.frontiersToVV(version));
          if (order !== -1 && order !== 0) continue;
          const full = new LoroDoc();
          full.import(doc.export({ mode: "update" }));
          full.checkout(version);
          shallow.checkout(version);
          expect(shallow.toJSON()).toEqual(full.toJSON());
        }
        shallow.checkoutToLatest();
        expect(shallow.toJSON()).toEqual(doc.toJSON());
      }
    }
  });
});
