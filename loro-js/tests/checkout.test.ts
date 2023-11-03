import { describe, expect, it } from "vitest";
import {
  Loro,
  setPanicHook,
} from "../src";

setPanicHook();
describe("Checkout", () => {
  it("simple checkout", () => {
    const doc = new Loro();
    const text = doc.getText("text");
    text.insert(0, "hello world");
    doc.commit();
    const v = doc.frontiers();
    text.insert(0, "000");
    expect(doc.toJson()).toStrictEqual({
      text: "000hello world"
    });

    doc.checkout(v);
    expect(doc.toJson()).toStrictEqual({
      text: "hello world"
    });
    v[0].counter -= 1;
    doc.checkout(v);
    expect(doc.toJson()).toStrictEqual({
      text: "hello worl"
    });
  });

  it("Chinese char", () => {
    const doc = new Loro();
    const text = doc.getText("text");
    text.insert(0, "你好世界");
    doc.commit();
    const v = doc.frontiers();
    expect(v[0].counter).toBe(3);
    v[0].counter -= 1;
    doc.checkout(v);
    expect(doc.toJson()).toStrictEqual({
      text: "你好世"
    });
    v[0].counter -= 1;
    doc.checkout(v);
    expect(doc.toJson()).toStrictEqual({
      text: "你好"
    });
    v[0].counter -= 1;
    doc.checkout(v);
    expect(doc.toJson()).toStrictEqual({
      text: "你"
    });
  })

  it("two clients", () => {
    const doc = new Loro();
    const text = doc.getText("text");
    text.insert(0, "0");
    doc.commit();

    const v0 = doc.frontiers();
    const docB = new Loro();
    docB.import(doc.exportFrom());
    expect(docB.cmpFrontiers(v0)).toBe(0);
    text.insert(1, "0");
    doc.commit();
    expect(docB.cmpFrontiers(doc.frontiers())).toBe(-1);
    const textB = docB.getText("text");
    textB.insert(0, "0");
    docB.commit();
    expect(docB.cmpFrontiers(doc.frontiers())).toBe(-1);
    docB.import(doc.exportFrom());
    expect(docB.cmpFrontiers(doc.frontiers())).toBe(1);
    doc.import(docB.exportFrom());
    expect(docB.cmpFrontiers(doc.frontiers())).toBe(0);
  });
});
