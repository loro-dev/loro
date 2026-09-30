import { describe, expect, it } from "vitest";
import { LoroDoc } from "../bundler/index";

// Several non-contiguous spans of one peer used to panic ("counter should be
// continuous") and trap the WASM instance. See loro-dev/loro#1155.
describe("export updates-in-range", () => {
  it("exports several non-contiguous spans of one peer", () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const text = doc.getText("t");
    for (let i = 0; i < 10; i++) {
      text.insert(i, String(i));
      doc.commit();
    }
    const bytes = doc.export({
      mode: "updates-in-range",
      spans: [
        { id: { peer: "1", counter: 0 }, len: 2 },
        { id: { peer: "1", counter: 5 }, len: 2 },
      ],
    });

    const target = new LoroDoc();
    target.import(
      doc.export({
        mode: "updates-in-range",
        spans: [{ id: { peer: "1", counter: 0 }, len: 5 }],
      }),
    );
    target.import(bytes);
    expect(target.getText("t").toString()).toBe("0123456");
    // The source doc is still usable.
    text.insert(0, "x");
    doc.commit();
    expect(doc.getText("t").toString()).toBe("x0123456789");
  });
});
