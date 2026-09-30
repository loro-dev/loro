import { describe, expect, it } from "vitest";
import { LoroDoc } from "../bundler/index";

// Two docs that shared a peer id wrote different ops under the same ids. Importing
// one into the other used to trap the WASM instance ("RuntimeError: unreachable")
// or scramble the text. It must throw and leave the doc unchanged and usable.
// See loro-dev/loro#1118 and context/import-peer-id-reuse.md.

function history(): LoroDoc {
  const doc = new LoroDoc();
  doc.setPeerId(7);
  doc.getMap("m").set("k", 0);
  doc.getText("t").insert(0, "a");
  doc.commit(); // 7@0, 7@1
  return doc;
}

function conflicting(text: string): LoroDoc {
  const doc = new LoroDoc();
  doc.setPeerId(7);
  doc.getText("t").insert(0, text);
  doc.commit();
  return doc;
}

function expectRejected(doc: LoroDoc, bytes: Uint8Array) {
  const value = doc.toJSON();
  const version = doc.version().toJSON();
  expect(() => doc.import(bytes)).toThrow(/has been used/);
  expect(doc.toJSON()).toEqual(value);
  expect(doc.version().toJSON()).toEqual(version);

  // Still usable: local edits and legitimate remote updates apply.
  doc.getText("after").insert(0, "local");
  doc.commit();
  const remote = new LoroDoc();
  remote.setPeerId(1000);
  remote.import(doc.export({ mode: "snapshot" }));
  remote.getMap("remote").set("k", "v");
  remote.commit();
  doc.import(remote.export({ mode: "update", from: doc.version() }));
  expect(doc.toJSON()).toEqual(remote.toJSON());
}

describe("importing history that reuses local op ids", () => {
  it("throws instead of trapping (issue repro)", () => {
    const other = conflicting("xyz"); // 7@0..=2
    expectRejected(history(), other.export({ mode: "update" }));
    expectRejected(history(), other.export({ mode: "snapshot" }));
  });

  it("throws instead of scrambling the text", () => {
    const doc = new LoroDoc();
    doc.setPeerId(7);
    doc.getText("t").insert(0, "hello");
    doc.commit();
    expectRejected(doc, conflicting("hellO world").export({ mode: "update" }));
    expect(doc.getText("t").toString()).toBe("hello");
  });

  it("still accepts overlapping re-imports of the same history", () => {
    const source = new LoroDoc();
    source.setPeerId(1);
    const text = source.getText("t");
    text.insert(0, "hello world");
    source.commit();
    const target = new LoroDoc();
    target.import(source.export({ mode: "update" }));
    text.delete(0, 6);
    text.insert(0, "hi ");
    source.commit();
    target.import(source.export({ mode: "update" }));
    target.import(source.export({ mode: "snapshot" }));
    expect(target.toJSON()).toEqual(source.toJSON());
  });
});
