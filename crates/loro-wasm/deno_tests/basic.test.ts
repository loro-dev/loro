import init, { initSync, LoroDoc } from "../web/loro_wasm.js";
import { expect } from "npm:expect";

await init();

Deno.test("basic", () => {
    const doc = new LoroDoc();
    doc.getText("text").insert(0, "Hello, world!");
    expect(doc.getText("text").toString()).toBe("Hello, world!");
});

Deno.test("fork when detached", () => {
    const doc = new LoroDoc();
    doc.setPeerId("0");
    doc.getText("text").insert(0, "Hello, world!");
    doc.checkout([{ peer: "0", counter: 5 }]);
    const newDoc = doc.fork();
    newDoc.setPeerId("1");
    newDoc.getText("text").insert(6, " Alice!");
    // ┌───────────────┐     ┌───────────────┐
    // │    Hello,     │◀─┬──│     world!    │
    // └───────────────┘  │  └───────────────┘
    //                    │
    //                    │  ┌───────────────┐
    //                    └──│     Alice!    │
    //                       └───────────────┘
    doc.import(newDoc.export({ mode: "update" }));
    doc.checkoutToLatest();
    console.log(doc.getText("text").toString()); // "Hello, world! Alice!"
});
