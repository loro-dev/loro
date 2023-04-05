import { describe, expect, it } from "vitest";
import {
  Delta,
  ListDiff,
  Loro,
  LoroEvent,
  MapDiff as MapDiff,
  TextDiff,
} from "../src";

describe("Frontiers", () => {
  it("two clients", () => {
    const doc = new Loro();
    const text = doc.getText("text");
    text.insert(doc, 0, "0");
    const v0 = doc.frontiers();
    const docB = new Loro();
    docB.import(doc.exportFrom());
    expect(docB.cmpFrontiers(v0)).toBe(0);
    text.insert(doc, 1, "0");
    expect(docB.cmpFrontiers(doc.frontiers())).toBe(-1);
    const textB = docB.getText("text");
    textB.insert(docB, 0, "0");
    expect(docB.cmpFrontiers(doc.frontiers())).toBe(-1);
    docB.import(doc.exportFrom());
    expect(docB.cmpFrontiers(doc.frontiers())).toBe(1);
    doc.import(docB.exportFrom());
    expect(docB.cmpFrontiers(doc.frontiers())).toBe(0);
  });
});
