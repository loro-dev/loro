import { describe, expect, test } from "vitest";

import {
  decodeFastSnapshot,
  decodePostcardVersionVector,
  decodeSstable,
  encodeFastSnapshot,
  encodePostcardVersionVector,
  encodeSstable,
} from "../src/codec";
import { LoroDoc, type JsonOpContent, type JsonSchema } from "../src/index";

/**
 * Port of Rust's crates/loro/tests/movable_list_invalid_ops.rs (loro-dev/loro#1125).
 * Forged movable-list moves and sets without a meaning are rejected and leave
 * the document untouched and usable; a move or set of a deleted element is
 * applied like a concurrent one, identically on every import path.
 * See context/loro-js-movable-list.md.
 */

/** Element IDs below are Rust's `L{lamport}@{peer index}` into the change's `peers`. */
type ForgedContent = Record<string, unknown>;

/** Peer 1 inserts [a, b, c] (lamports 0..=2), then deletes b (counter 3): [a, c]. */
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

/** One change by peer 1, causally after `base()`, holding one list op. */
function forged(content: ForgedContent): JsonSchema {
  return {
    schema_version: 1,
    start_version: {},
    peers: ["1", "2"],
    changes: [
      {
        id: "4@0",
        timestamp: 0,
        deps: ["3@0"],
        lamport: 4,
        msg: null,
        ops: [
          {
            container: "cid:root-list:MovableList",
            counter: 4,
            content: content as JsonOpContent,
          },
        ],
      },
    ],
  };
}

const moveDeleted = { type: "move", from: 0, to: 1, elem_id: "L1@0" };

interface Snapshot {
  readonly value: unknown;
  readonly version: [string, number][];
}

function snapshotOf(doc: LoroDoc): Snapshot {
  return {
    value: doc.toJSON(),
    version: [...doc.oplogVersion().toJSON()].sort(([left], [right]) =>
      left.localeCompare(right),
    ),
  };
}

/** The document still accepts local edits and valid remote updates. */
function assertUsable(doc: LoroDoc): void {
  const list = doc.getMovableList("list");
  const length = list.length;
  list.push("local");
  list.move(0, list.length - 1);
  doc.commit();
  expect(list.length).toBe(length + 1);
  const remote = new LoroDoc();
  remote.setPeerId(9);
  remote.import(doc.export({ mode: "snapshot" }));
  remote.getMovableList("list").push("remote");
  remote.commit();
  doc.import(remote.export({ mode: "update", from: doc.oplogVersion() }));
  expect(doc.toJSON()).toEqual(remote.toJSON());
}

/** `base()` plus an edit by peer 2 that the forged op (deps `3@0`) cannot see. */
function baseWithConcurrentEdit(): LoroDoc {
  const doc = base();
  doc.setPeerId(2);
  doc.getMovableList("list").insert(0, "x");
  doc.commit();
  return doc;
}

function assertRejected(doc: LoroDoc, json: JsonSchema): void {
  const before = snapshotOf(doc);
  expect(() => doc.importJsonUpdates(json)).toThrow(/causal history|out of range/u);
  expect(snapshotOf(doc)).toEqual(before);
  assertUsable(doc);
}

/** Imports `json` onto `base()` through every path; they must agree with a full replay. */
function assertSameResultOnEveryImportPath(json: JsonSchema, expected: unknown): void {
  const carrier = new LoroDoc();
  carrier.import(base().export({ mode: "update" }));
  // Detached imports only touch history, which gives us the op as binary.
  carrier.detach();
  carrier.importJsonUpdates(json);
  const update = carrier.export({
    mode: "updates-in-range",
    spans: [{ id: { peer: "1", counter: 4 }, len: 1 }],
  });
  const all = carrier.export({ mode: "update" });

  const replayed = new LoroDoc();
  replayed.import(all);
  expect(replayed.toJSON()).toEqual(expected);

  const fromSnapshot = new LoroDoc();
  fromSnapshot.import(base().export({ mode: "snapshot" }));
  const detached = base();
  detached.detach();
  const docs = [base(), base(), base(), fromSnapshot, detached];
  docs[0]!.importJsonUpdates(json);
  docs[1]!.import(update);
  docs[2]!.importBatch([update, update]);
  docs[3]!.importJsonUpdates(json);
  docs[4]!.import(update);
  docs[4]!.attach();
  for (const doc of docs) {
    expect(doc.toJSON()).toEqual(replayed.toJSON());
    expect(snapshotOf(doc).version).toEqual(snapshotOf(replayed).version);
    assertUsable(doc);
  }
}

describe("movable-list moves and sets from other peers", () => {
  test("a move of a deleted element is applied like a concurrent move", () => {
    // `from: 0` points at `a` in the op's version, so `a`'s position is consumed
    // and `b` comes back at index 1, exactly as for a concurrent move.
    assertSameResultOnEveryImportPath(forged(moveDeleted), { list: ["c", "b"] });
  });

  test("a set of a deleted element is applied like a concurrent set", () => {
    assertSameResultOnEveryImportPath(
      forged({ type: "set", elem_id: "L1@0", value: "z" }),
      { list: ["a", "c"] },
    );
  });

  test("a move or set of an unknown element is rejected", () => {
    const ops = [
      { type: "move", from: 0, to: 1, elem_id: "L99@0" },
      { type: "set", elem_id: "L99@0", value: "z" },
      // Peer 2 has no ops at all in `base()`.
      { type: "move", from: 0, to: 1, elem_id: "L0@1" },
      // Lamport 3 is `base()`'s delete op, not an element.
      { type: "set", elem_id: "L3@0", value: "z" },
    ];
    for (const op of ops) {
      assertRejected(base(), forged(op));
      assertRejected(baseWithConcurrentEdit(), forged(op));
    }
  });

  test("a move of an element from another container is rejected", () => {
    const doc = base();
    doc.getMovableList("other").insert(0, "o");
    doc.commit();
    // `other`'s element is `L4@0`; target it from `list` in a change after it.
    const json: JsonSchema = {
      schema_version: 1,
      start_version: {},
      peers: ["1"],
      changes: [
        {
          id: "5@0",
          timestamp: 0,
          deps: ["4@0"],
          lamport: 5,
          msg: null,
          ops: [
            {
              container: "cid:root-list:MovableList",
              counter: 5,
              content: { type: "move", from: 0, to: 1, elem_id: "L4@0" } as JsonOpContent,
            },
          ],
        },
      ],
    };
    assertRejected(doc, json);
  });

  test("a move or set on a snapshot-imported document is checked", () => {
    const snapshot = base().export({ mode: "snapshot" });
    for (const op of [
      { type: "move", from: 0, to: 1, elem_id: "L99@0" },
      { type: "set", elem_id: "L3@0", value: "z" },
    ]) {
      assertRejected(LoroDoc.fromSnapshot(snapshot), forged(op));
    }
    const doc = LoroDoc.fromSnapshot(snapshot);
    doc.importJsonUpdates(forged({ type: "move", from: 1, to: 0, elem_id: "L2@0" }));
    expect(doc.toJSON()).toEqual({ list: ["c", "a"] });
  });

  test("a move of a snapshot element does not decode the snapshot's history", () => {
    const source = base();
    const snapshot = decodeFastSnapshot(source.export({ mode: "snapshot" }));
    // Decoding this snapshot's history fails, since its version vector claims
    // one more op than the history holds.
    const oplog = decodeSstable(snapshot.oplog).map((entry) => {
      if (entry.key.length !== 2 || entry.key[0] !== 0x76 || entry.key[1] !== 0x76) {
        return entry;
      }
      const version = decodePostcardVersionVector(entry.value).map((id) => ({
        ...id,
        counter: id.counter + 1,
      }));
      return { ...entry, value: encodePostcardVersionVector(version) };
    });
    const doc = LoroDoc.fromSnapshot(
      encodeFastSnapshot({
        ...snapshot,
        oplog: encodeSstable(oplog, { compression: "none" }),
      }),
    );
    // Peer 2's edit, since the forged version already claims peer 1's next op.
    const version = source.oplogVersion();
    source.setPeerId(2);
    source.getMovableList("list").move(1, 0);
    source.getMovableList("list").set(0, "C");
    source.commit();

    doc.import(source.export({ mode: "update", from: version }));
    expect(doc.toJSON()).toEqual({ list: ["C", "a"] });
    expect(() => doc.changeCount()).toThrow(/version does not match/u);
  });

  test("a move of an element outside the op's causal history is rejected", () => {
    // Peer 2 inserts `y` concurrently with `base()`'s delete; the forged op by
    // peer 1 depends only on `3@0` and so cannot have seen `y` (`L3@1`).
    const doc = base();
    const beforeDelete = new LoroDoc();
    beforeDelete.setPeerId(2);
    beforeDelete.import(
      doc.export({
        mode: "updates-in-range",
        spans: [{ id: { peer: "1", counter: 0 }, len: 3 }],
      }),
    );
    beforeDelete.getMovableList("list").push("y");
    beforeDelete.commit();
    doc.import(beforeDelete.export({ mode: "update" }));
    expect(doc.toJSON()).toEqual({ list: ["a", "c", "y"] });

    assertRejected(doc, forged({ type: "move", from: 0, to: 1, elem_id: "L3@1" }));
  });

  test("a move with an out-of-range index is rejected", () => {
    const ops = [
      { type: "move", from: 9, to: 0, elem_id: "L0@0" },
      { type: "move", from: 0, to: 9, elem_id: "L0@0" },
      { type: "move", from: 5, to: 1, elem_id: "L1@0" },
    ];
    for (const op of ops) {
      assertRejected(base(), forged(op));
      assertRejected(baseWithConcurrentEdit(), forged(op));
    }
  });

  test("a detached import of an unknown element is rejected", () => {
    const doc = base();
    doc.detach();
    const before = snapshotOf(doc);
    expect(() =>
      doc.importJsonUpdates(forged({ type: "move", from: 0, to: 1, elem_id: "L99@0" })),
    ).toThrow(/causal history/u);
    expect(snapshotOf(doc)).toEqual(before);
    doc.attach();
    assertUsable(doc);
  });

  test("importBatch with an out-of-range move rolls back and stays attached", () => {
    // Detached imports only touch history (bounds are checked when the document
    // reattaches), which lets us export the invalid op as binary.
    const carrier = base();
    carrier.detach();
    carrier.importJsonUpdates(forged({ type: "move", from: 9, to: 0, elem_id: "L0@0" }));
    const forgedUpdate = carrier.export({
      mode: "updates-in-range",
      spans: [{ id: { peer: "1", counter: 4 }, len: 1 }],
    });

    const doc = base();
    const valid = new LoroDoc();
    valid.setPeerId(5);
    valid.import(doc.export({ mode: "update" }));
    valid.getMovableList("list").push("v");
    valid.commit();
    const validUpdate = valid.export({ mode: "update", from: doc.oplogVersion() });

    const before = snapshotOf(doc);
    expect(() => doc.import(forgedUpdate)).toThrow(/out of range/u);
    expect(snapshotOf(doc)).toEqual(before);
    expect(() => doc.importBatch([validUpdate, forgedUpdate])).toThrow(/out of range/u);
    expect(doc.isDetached()).toBe(false);
    expect(snapshotOf(doc)).toEqual(before);
    assertUsable(doc);
  });

  test("moves and sets of shallow-root elements still import", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const list = doc.getMovableList("list");
    list.insert(0, "a");
    list.insert(1, "b");
    list.insert(2, "c");
    doc.commit();
    list.insert(3, "d");
    doc.commit();

    const shallow = new LoroDoc();
    shallow.import(
      doc.export({ mode: "shallow-snapshot", frontiers: doc.oplogFrontiers() }),
    );
    const version = shallow.oplogVersion();

    // Elements created before the shallow root are moved/set after it.
    list.move(0, 3);
    list.set(0, "B");
    doc.commit();
    shallow.import(doc.export({ mode: "update", from: version }));
    expect(shallow.toJSON()).toEqual(doc.toJSON());

    // Unknown elements are still rejected on a shallow document.
    const json: JsonSchema = {
      schema_version: 1,
      start_version: {},
      peers: ["1"],
      changes: [
        {
          id: "6@0",
          timestamp: 0,
          deps: ["5@0"],
          lamport: 6,
          msg: null,
          ops: [
            {
              container: "cid:root-list:MovableList",
              counter: 6,
              content: {
                type: "move",
                from: 0,
                to: 1,
                elem_id: "L99@0",
              } as JsonOpContent,
            },
          ],
        },
      ],
    };
    assertRejected(shallow, json);
  });

  test("a move of an element deleted before the shallow root is rejected", () => {
    // The shallow history only keeps elements alive at the root, and no valid op
    // after the root can see `b`, which was deleted before it.
    const source = base();
    const shallowSnapshot = source.export({
      mode: "shallow-snapshot",
      frontiers: source.oplogFrontiers(),
    });
    for (const op of [
      moveDeleted,
      { type: "set", elem_id: "L1@0", value: "z" },
      { type: "move", from: 0, to: 1, elem_id: "L99@0" },
    ]) {
      const doc = new LoroDoc();
      doc.import(shallowSnapshot);
      assertRejected(doc, forged(op));
    }

    // `a` is alive at the root, so it can still be moved and set.
    const doc = new LoroDoc();
    doc.import(shallowSnapshot);
    doc.importJsonUpdates(forged({ type: "set", elem_id: "L0@0", value: "A" }));
    expect(doc.toJSON()).toEqual({ list: ["A", "c"] });
  });
});
