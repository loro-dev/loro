import init, { initSync, LoroDoc } from "../web/loro_wasm.js";
import { expect } from "npm:expect";

await init();

Deno.test("basic", () => {
    const doc = new LoroDoc();
    doc.getText("text").insert(0, "Hello, world!");
    expect(doc.getText("text").toString()).toBe("Hello, world!");
});
