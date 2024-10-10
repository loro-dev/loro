import { describe, expect, it } from "vitest";
import { CounterDiff, LoroDoc } from "../src";

function oneMs(): Promise<void> {
  return new Promise((r) => setTimeout(r));
}

describe("counter", () => {
  it("increment", () => {
    const doc = new LoroDoc();
    const counter = doc.getCounter("counter");
    counter.increment(1);
    counter.increment(2);
    counter.decrement(1);
    expect(counter.value).toBe(2);
  });

   it("encode", async () => {
    const doc = new LoroDoc();
    const counter = doc.getCounter("counter");
    counter.increment(1);
    counter.increment(2);
    counter.decrement(4);
    
    const updates = doc.exportFrom();
    const snapshot = doc.exportSnapshot();
    const json = doc.exportJsonUpdates();
    const doc2 = new LoroDoc();
    doc2.import(updates);
    expect(doc2.toJSON()).toStrictEqual(doc.toJSON());
    const doc3 = new LoroDoc();
    doc3.import(snapshot);
    expect(doc3.toJSON()).toStrictEqual(doc.toJSON());
    const doc4 = new LoroDoc();
    doc4.importJsonUpdates(json);
    expect(doc4.toJSON()).toStrictEqual(doc.toJSON());
  });
});

describe("counter event", () => {
  it("event", async () => {
    const doc = new LoroDoc();
    let triggered = false;
    doc.subscribe((e) => {
      triggered = true;
      const diff = e.events[0].diff as CounterDiff;
      expect(diff.type).toBe("counter");
      expect(diff.increment).toStrictEqual(-1);
    });
    const counter = doc.getCounter("counter");

    counter.increment(1);
    counter.increment(2);
    counter.decrement(4);
    doc.commit();
    await oneMs();
    expect(triggered).toBe(true);
  });
});

