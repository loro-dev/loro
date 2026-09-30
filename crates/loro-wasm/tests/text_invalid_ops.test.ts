import { describe, expect, it } from "vitest";
import { LoroDoc } from "../bundler/index";

// A Text op past the end of the text used to panic inside the doc locks, which
// traps the WASM instance ("RuntimeError: unreachable") (loro-dev/loro#1160). It
// must throw a readable error and leave the doc unchanged and usable.

/** Peer 1 writes "ab" (counters/lamports 0..=1). */
function base(): LoroDoc {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  doc.getText("t").insert(0, "ab");
  doc.commit();
  return doc;
}

function forged(ops: unknown[]): string {
  return JSON.stringify({
    schema_version: 1,
    start_version: {},
    peers: ["1"],
    changes: [
      {
        id: "2@0",
        timestamp: 0,
        deps: ["1@0"],
        lamport: 2,
        msg: null,
        ops: ops.map((content, i) => ({
          container: "cid:root-t:Text",
          content,
          counter: 2 + i,
        })),
      },
    ],
  });
}

function expectRejected(doc: LoroDoc, json: string) {
  const value = doc.toJSON();
  const delta = doc.getText("t").toDelta();
  const vv = doc.oplogVersion().toJSON();
  let error: unknown;
  try {
    doc.importJsonUpdates(json);
  } catch (e) {
    error = e;
  }
  expect(error).toBeDefined();
  expect(String(error)).toMatch(/Decode error/);
  expect(String(error)).not.toMatch(/unreachable/);
  expect(doc.toJSON()).toEqual(value);
  expect(doc.getText("t").toDelta()).toEqual(delta);
  expect(doc.oplogVersion().toJSON()).toEqual(vv);

  // Still usable.
  const text = doc.getText("t");
  text.insert(text.length, "!");
  doc.commit();
  const remote = new LoroDoc();
  remote.import(doc.export({ mode: "snapshot" }));
  remote.getText("t").insert(0, "r");
  remote.commit();
  doc.import(remote.export({ mode: "update", from: doc.oplogVersion() }));
  expect(doc.toJSON()).toEqual(remote.toJSON());
}

describe("out-of-bounds text ops", () => {
  it("rejects an insert past the end of the text", () => {
    expectRejected(base(), forged([{ type: "insert", pos: 3, text: "x" }]));
  });

  it("rejects deletes and marks past the end of the text", () => {
    expectRejected(
      base(),
      forged([{ type: "delete", pos: 1, len: 2, start_id: "1@0" }]),
    );
    expectRejected(
      base(),
      forged([
        {
          type: "mark",
          start: 1,
          end: 9,
          style_key: "bold",
          style_value: true,
          info: 0,
        },
        { type: "mark_end" },
      ]),
    );
  });

  it("still imports an insert at the end of the text", () => {
    const doc = base();
    doc.importJsonUpdates(forged([{ type: "insert", pos: 2, text: "x" }]));
    expect(doc.getText("t").toString()).toBe("abx");
  });
});
