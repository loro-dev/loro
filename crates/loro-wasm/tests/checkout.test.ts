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
    docB.import(doc.exportFrom());
    expect(docB.cmpWithFrontiers(v0)).toBe(0);
    text.insert(1, "0");
    doc.commit();
    expect(docB.cmpWithFrontiers(doc.frontiers())).toBe(-1);
    const textB = docB.getText("text");
    textB.insert(0, "0");
    docB.commit();
    expect(docB.cmpWithFrontiers(doc.frontiers())).toBe(-1);
    docB.import(doc.exportFrom());
    expect(docB.cmpWithFrontiers(doc.frontiers())).toBe(1);
    doc.import(docB.exportFrom());
    expect(docB.cmpWithFrontiers(doc.frontiers())).toBe(0);
  });
});
