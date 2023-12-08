import { describe, expect, it } from "vitest";
import {
  Loro,
} from "../src";

describe("Checkout", () => {
  it("simple checkout", () => {
    const doc = new Loro();
    doc.setPeerId(0n);
    const text = doc.getText("text");
    text.insert(0, "H");
    doc.commit();
    const v = doc.frontiers();
    text.insert(1, "i");
    expect(doc.toJson()).toStrictEqual({
      text: "Hi"
    });

    doc.checkout([{ peer: 0n, counter: 0 }]);
    expect(doc.toJson()).toStrictEqual({
      text: "H"
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
