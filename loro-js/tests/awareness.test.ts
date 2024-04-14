import { describe, expect, it } from "vitest";
import { Awareness, setDebug } from "../src/index";

describe("Awareness", () => {
  it("setLocalRecord", () => {
    const awareness = new Awareness("123", 30_000);
    awareness.setLocalRecord("foo", "bar");
    expect(awareness.getRecord("123")).toEqual({ foo: "bar" });
    expect(awareness.getAllRecords()).toEqual({ "123": { foo: "bar" } });
  });

  it("sync", () => {
    const awareness = new Awareness("123", 30_000);
    awareness.setLocalRecord("foo", "bar");

    const awarenessB = new Awareness("223", 30_000);
    const changed = awarenessB.apply(awareness.encode(["123"]));

    expect(changed).toStrictEqual(["123"]);
    expect(awarenessB.getRecord("123")).toEqual({ foo: "bar" });
    expect(awarenessB.getAllRecords()).toEqual({ "123": { foo: "bar" } });
  });

  it("not sync if peer is not in sync list", () => {
    const awareness = new Awareness("123", 30_000);
    awareness.setLocalRecord("foo", "bar");

    const awarenessB = new Awareness("223", 30_000);
    awarenessB.apply(awareness.encode(["123"]));
    awarenessB.setLocalRecord("new", "bee");

    const awarenessC = new Awareness("323", 30_000);
    const changed = awarenessC.apply(awarenessB.encode(["223"]));
    expect(changed).toStrictEqual(["223"]);

    expect(awarenessC.getRecord("223")).toEqual({ new: "bee" });
    expect(awarenessC.getAllRecords()).toEqual({ "223": { new: "bee" } });
  });

  it("should remove outdated", async () => {
    setDebug();
    const awareness = new Awareness("123", 5);
    awareness.setLocalRecord("foo", "bar");
    await new Promise((r) => setTimeout(r, 10));
    const outdated = awareness.removeOutdated();
    expect(outdated).toEqual(["123"]);
    expect(awareness.getAllRecords()).toEqual({});
  });
});
