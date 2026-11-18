import { expect, it } from "vitest";
import { LoroDoc, LoroText } from "../bundler/index";

it("keeps UTF-8 and UTF-16 text semantics on both sides of the short-string limit", () => {
  const doc = new LoroDoc();
  const text = doc.getText("text");
  const sample = ["", "A", "中", "\u0000", "\ud800", "\udfff", "\uffff", "ASCII", "中文", "😀", "\ud800A\udc00", "中".repeat(42), "中".repeat(43)];
  const decoded = (input: string) => new TextDecoder().decode(new TextEncoder().encode(input));

  try {
    for (const value of sample) {
      const next = decoded(value);
      text.insert(0, value);
      expect(text.toString()).toBe(next);
      text.insertUtf8(new TextEncoder().encode(next).length, value);
      expect(text.toString()).toBe(next + next);
      expect(text.splice(0, next.length * 2, "")).toBe(next + next);
      expect(text.toString()).toBe("");
      expect(text.splice(0, 0, value)).toBe("");
      expect(text.toString()).toBe(next);
      text.delete(0, next.length);
    }

    expect(() => text.insert(0, 42 as unknown as string)).toThrow("Expected a string");
    expect(text.toString()).toBe("");
  } finally {
    text.free();
    doc.free();
  }
});

it("returns an ordinary Map and full decimal peer ID for a single-peer import", () => {
  const source = new LoroDoc();
  const replica = new LoroDoc();
  const text: LoroText = source.getText("text");
  try {
    const peer = "18446744073709551614";
    source.setPeerId(peer);
    text.insert(0, "字😀");
    const { success, pending } = replica.import(source.export({ mode: "update" }));
    expect(success).toBeInstanceOf(Map);
    expect([...success]).toEqual([[peer, { start: 0, end: 2 }]]);
    expect(pending).toBeNull();
  } finally {
    text.free();
    source.free();
    replica.free();
  }
});
