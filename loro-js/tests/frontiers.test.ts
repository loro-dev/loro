import { describe, expect, it } from "vitest";
import {
  Loro,
  setPanicHook,
} from "../src";

setPanicHook();
describe("Frontiers", () => {
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
