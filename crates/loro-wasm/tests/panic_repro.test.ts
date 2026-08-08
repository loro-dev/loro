import { describe, expect, it } from "vitest";
import { LoroDoc, LoroText } from "../bundler/index";

// Regression tests: these APIs used to leak Rust panics to JS as an unreadable
// `RuntimeError: unreachable executed` (with no message and a corrupted wasm
// instance). They must now throw catchable JS errors with a readable message,
// or return `undefined`.
describe("wasm panic regressions", () => {
  it("LoroText.getCursor with an invalid side throws a readable error", () => {
    const doc = new LoroDoc();
    const text = doc.getText("text");
    text.insert(0, "abc");
    expect(() => text.getCursor(0, 2 as never)).toThrowError(
      /Side must be -1 \| 0 \| 1/,
    );
    expect(() => text.getCursor(0, true as never)).toThrowError(
      /Side must be -1 \| 0 \| 1/,
    );
    // Only `undefined` selects the default; other falsy or non-integer
    // values must not be silently coerced.
    for (const bad of [0.5, Number.NaN, null, false, "1"]) {
      expect(() => text.getCursor(0, bad as never)).toThrowError(
        /Side must be -1 \| 0 \| 1/,
      );
    }
    // Valid sides still work
    expect(text.getCursor(0, -1)).toBeDefined();
    expect(text.getCursor(0, 0)).toBeDefined();
    expect(text.getCursor(0, 1)).toBeDefined();
    expect(text.getCursor(0)).toBeDefined();
    expect(text.getCursor(0, undefined)).toBeDefined();
  });

  it("LoroList.getCursor with an invalid side throws a readable error", () => {
    const doc = new LoroDoc();
    const list = doc.getList("list");
    list.insert(0, "a");
    expect(() => list.getCursor(0, 2 as never)).toThrowError(
      /Side must be -1 \| 0 \| 1/,
    );
    expect(() => list.getCursor(0, {} as never)).toThrowError(
      /Side must be -1 \| 0 \| 1/,
    );
    expect(list.getCursor(0, 1)).toBeDefined();
    expect(list.getCursor(0)).toBeDefined();
  });

  it("LoroMovableList.getCursor with an invalid side throws a readable error", () => {
    const doc = new LoroDoc();
    const list = doc.getMovableList("list");
    list.insert(0, "a");
    expect(() => list.getCursor(0, -2 as never)).toThrowError(
      /Side must be -1 \| 0 \| 1/,
    );
    expect(() => list.getCursor(0, "x" as never)).toThrowError(
      /Side must be -1 \| 0 \| 1/,
    );
    expect(list.getCursor(0, 1)).toBeDefined();
    expect(list.getCursor(0)).toBeDefined();
  });

  it("LoroText.getEditorOf returns undefined for out-of-range positions", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1n);
    const text = doc.getText("text");
    text.insert(0, "abc");
    expect(text.getEditorOf(0)).toBe("1");
    // pos == length and empty text used to panic on `id.unwrap()`
    expect(text.getEditorOf(3)).toBeUndefined();
    const empty = new LoroDoc().getText("t");
    expect(empty.getEditorOf(0)).toBeUndefined();
  });

  it("accessing an unknown container throws a readable error instead of trapping", () => {
    // Craft a document containing an unknown container (as produced by a
    // future Loro version) via JSON updates: take a real export and rewrite
    // the child container id to an unknown container type.
    const doc = new LoroDoc();
    doc.setPeerId(1n);
    const map = doc.getMap("map");
    map.setContainer("k", new LoroText());
    const json = doc.exportJsonUpdates();
    const textCid = "cid:0@0:Text";
    expect(JSON.stringify(json)).toContain(textCid);
    const patched = JSON.parse(
      JSON.stringify(json).split(textCid).join("cid:0@0:Unknown(6)"),
    );

    const doc2 = new LoroDoc();
    doc2.importJsonUpdates(patched);
    const map2 = doc2.getMap("map");
    // Used to hit `unreachable!()` in handler_to_js_value and trap the wasm
    // instance; now it must throw a catchable, readable error.
    expect(() => map2.get("k")).toThrowError(/[Uu]nknown container/);
    // Deep-value paths should not trap either.
    expect(() => doc2.getDeepValueWithID()).not.toThrow();
  });

  it("getByPath through unknown/leaf containers returns undefined, not a trap", () => {
    // Unknown containers are opaque: descending below them has no match.
    const doc = new LoroDoc();
    doc.setPeerId(1n);
    const map = doc.getMap("map");
    map.setContainer("k", new LoroText());
    const json = doc.exportJsonUpdates();
    const textCid = "cid:0@0:Text";
    const patched = JSON.parse(
      JSON.stringify(json).split(textCid).join("cid:0@0:Unknown(6)"),
    );
    const doc2 = new LoroDoc();
    doc2.importJsonUpdates(patched);
    expect(doc2.getByPath("map/k/foo")).toBeUndefined();
    expect(doc2.getByPath("map/k/foo/1")).toBeUndefined();
    // The unknown container itself is still a readable error.
    expect(() => doc2.getByPath("map/k")).toThrowError(
      /[Uu]nknown container/,
    );

    // Leaf containers (e.g. counters) have no addressable children either.
    const doc3 = new LoroDoc();
    doc3.getCounter("c").increment(1);
    expect(doc3.getByPath("c/x")).toBeUndefined();
    expect(doc3.getByPath("c/x/y")).toBeUndefined();
    expect(doc3.getByPath("c")).toBeDefined();
  });
});
