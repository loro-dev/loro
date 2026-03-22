import { describe, expect, it } from "vitest";
import { LoroDoc } from "../bundler/index";

describe("Checkout", () => {
  it("simple checkout", async () => {
    const doc = new LoroDoc();
    doc.setPeerId(0n);
    const text = doc.getText("text");
    text.insert(0, "H");
    doc.commit();
    let triggered = false;
    doc.subscribe((e) => {
      expect(e.by).not.toBe("import");
      expect(e.by === "checkout" || e.by === "local").toBeTruthy();
      triggered = true;
    });
    const v = doc.frontiers();
    text.insert(1, "i");
    expect(doc.toJSON()).toStrictEqual({
      text: "Hi",
    });

    expect(doc.isDetached()).toBeFalsy();
    doc.checkout([{ peer: "0", counter: 0 }]);
    expect(doc.isDetached()).toBeTruthy();
    expect(doc.toJSON()).toStrictEqual({
      text: "H",
    });
    await new Promise((resolve) => setTimeout(resolve, 1));
    expect(triggered).toBeTruthy();
  });

  it("Chinese char", () => {
    const doc = new LoroDoc();
    const text = doc.getText("text");
    text.insert(0, "你好世界");
    doc.commit();
    const v = doc.frontiers();
    expect(v[0].counter).toBe(3);
    v[0].counter -= 1;
    doc.checkout(v);
    expect(doc.toJSON()).toStrictEqual({
      text: "你好世",
    });
    v[0].counter -= 1;
    doc.checkout(v);
    expect(doc.toJSON()).toStrictEqual({
      text: "你好",
    });
    v[0].counter -= 1;
    doc.checkout(v);
    expect(doc.toJSON()).toStrictEqual({
      text: "你",
    });
  });

  it("two clients", () => {
    const doc = new LoroDoc();
    const text = doc.getText("text");
    text.insert(0, "0");
    doc.commit();

    const v0 = doc.frontiers();
    const docB = new LoroDoc();
    docB.import(doc.export({ mode: "update" }));
    expect(docB.cmpWithFrontiers(v0)).toBe(0);
    text.insert(1, "0");
    doc.commit();
    expect(docB.cmpWithFrontiers(doc.frontiers())).toBe(-1);
    const textB = docB.getText("text");
    textB.insert(0, "0");
    docB.commit();
    expect(docB.cmpWithFrontiers(doc.frontiers())).toBe(-1);
    docB.import(doc.export({ mode: "update" }));
    expect(docB.cmpWithFrontiers(doc.frontiers())).toBe(1);
    doc.import(docB.export({ mode: "update" }));
    expect(docB.cmpWithFrontiers(doc.frontiers())).toBe(0);
  });

  it("forkAt inside subscription during checkout event should not corrupt state", async () => {
    const doc = new LoroDoc();
    doc.setPeerId("1");

    // Make some changes
    doc.getText("text").insert(0, "Hello");
    doc.commit();
    const frontier1 = doc.frontiers(); // [{ peer: "1", counter: 4 }]

    doc.getText("text").insert(5, " World");
    doc.commit();

    // Verify initial state
    expect(doc.toJSON()).toStrictEqual({ text: "Hello World" });

    // Subscribe and call forkAt inside the callback
    let forkResult: any = null;
    doc.subscribe((event) => {
      if (event.by === "checkout") {
        // BUG: This corrupts the checkout state
        const fork = doc.forkAt(doc.frontiers());
        forkResult = fork.toJSON();
      }
    });

    // Checkout to earlier state
    doc.checkout(frontier1);

    // Wait for events to be processed
    await new Promise((resolve) => setTimeout(resolve, 1));

    expect(doc.frontiers()).toStrictEqual(frontier1);
    expect(doc.toJSON()).toStrictEqual({ text: "Hello" });

    // The fork should also have the correct state at frontier1
    expect(forkResult).toStrictEqual({ text: "Hello" });
  });
});
