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
    const txn = doc.newTransaction("");
    text.insert(txn, 0, "0");
    txn.commit();

    const v0 = doc.frontiers();
    const docB = new Loro();
    docB.import(doc.exportFrom());
    expect(docB.cmpFrontiers(v0)).toBe(0);
    doc.transact((t) => {
      text.insert(t, 1, "0");
    });
    expect(docB.cmpFrontiers(doc.frontiers())).toBe(-1);
    const textB = docB.getText("text");
    docB.transact((t) => {
      textB.insert(t, 0, "0");
    });
    expect(docB.cmpFrontiers(doc.frontiers())).toBe(-1);
    docB.import(doc.exportFrom());
    expect(docB.cmpFrontiers(doc.frontiers())).toBe(1);
    doc.import(docB.exportFrom());
    expect(docB.cmpFrontiers(doc.frontiers())).toBe(0);
  });
});
