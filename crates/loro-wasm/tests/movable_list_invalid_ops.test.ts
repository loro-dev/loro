import { describe, expect, it } from "vitest";
import { LoroDoc } from "../bundler/index";

// Malformed movable-list ops used to panic inside the doc locks, which traps the
// WASM instance ("RuntimeError: unreachable"). They must throw a readable error
// and leave the doc unchanged and usable. See context/movable-list-op-validation.md.

/** Peer 1: list = [a, b, c] (lamports 0..=2), then delete `b` (counter 3). */
function base(): LoroDoc {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  const list = doc.getMovableList("list");
  list.insert(0, "a");
  list.insert(1, "b");
  list.insert(2, "c");
  doc.commit();
  list.delete(1, 1);
  doc.commit();
  return doc;
}

function forged(content: unknown): string {
  return JSON.stringify({
    schema_version: 1,
    start_version: {},
    peers: ["1"],
    changes: [
      {
        id: "4@0",
        timestamp: 0,
        deps: ["3@0"],
        lamport: 4,
        msg: null,
        ops: [{ container: "cid:root-list:MovableList", content, counter: 4 }],
      },
    ],
  });
}

/** A map-only change, then `content` in a change that depends on it. */
function forgedAfterMapChange(container: string, content: unknown): string {
  return JSON.stringify({
    schema_version: 1,
    start_version: {},
    peers: ["1"],
    changes: [
      {
        id: "4@0",
        timestamp: 0,
        deps: ["3@0"],
        lamport: 4,
        msg: null,
        ops: [
          {
            container: "cid:root-meta:Map",
            content: { type: "insert", key: "k", value: 1 },
            counter: 4,
          },
        ],
      },
      {
        id: "5@0",
        timestamp: 0,
        deps: ["4@0"],
        lamport: 5,
        msg: null,
        ops: [{ container, content, counter: 5 }],
      },
    ],
  });
}

function expectRejected(doc: LoroDoc, json: string) {
  const value = doc.toJSON();
  const vv = doc.oplogVersion().toJSON();
  let error: unknown;
  try {
    doc.importJsonUpdates(json);
  } catch (e) {
    error = e;
  }
  // LoroError reaches JS as its message.
  expect(error).toBeDefined();
  expect(String(error)).toMatch(/Decode error/);
  expect(String(error)).not.toMatch(/unreachable/);
  expect(doc.toJSON()).toEqual(value);
  expect(doc.oplogVersion().toJSON()).toEqual(vv);

  // Still usable.
  const list = doc.getMovableList("list");
  list.push("local");
  doc.commit();
  const remote = new LoroDoc();
  remote.import(doc.export({ mode: "snapshot" }));
  remote.getMovableList("list").push("remote");
  remote.commit();
  doc.import(remote.export({ mode: "update", from: doc.oplogVersion() }));
  expect(doc.toJSON()).toEqual(remote.toJSON());
}

describe("malformed movable list ops", () => {
  it("rejects moves and sets of unknown elements", () => {
    expectRejected(
      base(),
      forged({ type: "move", from: 0, to: 1, elem_id: "L99@0" }),
    );
    expectRejected(
      base(),
      forged({ type: "set", elem_id: "L99@0", value: "z" }),
    );
  });

  it("rejects forged ops that depend on a change in the same import", () => {
    expectRejected(
      base(),
      forgedAfterMapChange("cid:root-list:MovableList", {
        type: "move",
        from: 0,
        to: 1,
        elem_id: "L99@0",
      }),
    );
    expectRejected(
      base(),
      forgedAfterMapChange("cid:root-list:MovableList", {
        type: "move",
        from: 9,
        to: 0,
        elem_id: "L0@0",
      }),
    );
    // `L0@0` belongs to `list`; moving it inside `other` must not put it in both.
    expectRejected(
      base(),
      forgedAfterMapChange("cid:root-other:MovableList", {
        type: "move",
        from: 0,
        to: 0,
        elem_id: "L0@0",
      }),
    );
  });

  it("rejects huge positions instead of trapping", () => {
    const edge = Math.floor(0xffffffff / 4);
    for (const content of [
      { type: "move", from: edge, to: 0, elem_id: "L0@0" },
      { type: "move", from: 0, to: edge, elem_id: "L0@0" },
      { type: "move", from: 0, to: 0xffffffff, elem_id: "L0@0" },
      { type: "insert", pos: edge, value: ["z"] },
      { type: "delete", pos: edge, len: 1, start_id: "0@0" },
    ]) {
      expectRejected(base(), forged(content));
    }
  });

  it("applies a move of a deleted element like a concurrent move", () => {
    const doc = base();
    doc.importJsonUpdates(
      forged({ type: "move", from: 0, to: 1, elem_id: "L1@0" }),
    );
    const replayed = new LoroDoc();
    replayed.import(doc.export({ mode: "update" }));
    expect(doc.toJSON()).toEqual({ list: ["c", "b"] });
    expect(replayed.toJSON()).toEqual(doc.toJSON());
  });
});

describe("docs loaded from a snapshot", () => {
  it("import moves and sets of old elements from other peers", () => {
    for (const set of [false, true]) {
      const p2 = new LoroDoc();
      p2.setPeerId(2);
      p2.getMovableList("list").insert(0, "a");
      p2.getMovableList("list").insert(1, "b");
      p2.commit();
      const p1 = new LoroDoc();
      p1.setPeerId(1);
      p1.import(p2.export({ mode: "update" }));
      p1.getMovableList("list").push("x");
      p1.commit();

      const b = new LoroDoc();
      b.import(p1.export({ mode: "snapshot" }));
      const vv = b.oplogVersion();
      if (set) {
        p1.getMovableList("list").set(0, "A");
      } else {
        p1.getMovableList("list").move(0, 2);
      }
      p1.commit();
      b.import(p1.export({ mode: "update", from: vv }));
      expect(b.toJSON()).toEqual(p1.toJSON());
    }
  });

  it("getChangeAtLamport finds changes stored in the snapshot", () => {
    const src = new LoroDoc();
    src.setPeerId(7);
    const text = src.getText("t");
    for (let i = 0; i < 300; i++) {
      text.insert(0, `${i}-abcdefghijklmnopqrstuvwxyz`);
      src.commit();
    }
    const end = src.oplogVersion().get("7")!;
    for (let lamport = 0; lamport < end; lamport += 97) {
      const loaded = new LoroDoc();
      loaded.import(src.export({ mode: "snapshot" }));
      const expected = src.getChangeAtLamport("7", lamport);
      const got = loaded.getChangeAtLamport("7", lamport);
      expect(expected).toBeDefined();
      expect(got?.counter).toBe(expected!.counter);
      expect(got?.lamport).toBe(expected!.lamport);
    }
  });
});
