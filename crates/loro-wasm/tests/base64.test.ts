import { describe, expect, it } from "vitest";

const loadBase64Module = async () => {
  const module = await import("../base64/index.js");
  return module;
};

describe("base64 build", () => {
  it("can mutate text", async () => {
    const { LoroDoc } = await loadBase64Module();
    const doc = new LoroDoc();

    const text = doc.getText("text");
    text.insert(0, "Hello, base64!");

    expect(text.toString()).toBe("Hello, base64!");
  });

  it("exposes version string", async () => {
    const { LORO_VERSION } = await loadBase64Module();

    expect(typeof LORO_VERSION).toBe("function");
    const version = LORO_VERSION();

    expect(typeof version).toBe("string");
    expect(version.length).toBeGreaterThan(0);
  });
});
