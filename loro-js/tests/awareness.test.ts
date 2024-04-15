import { describe, expect, it } from "vitest";
import { Awareness, AwarenessWasm, setDebug } from "../src/index";
import { AwarenessListener } from "../src/awareness";

describe("Awareness", () => {
  it("setLocalRecord", () => {
    const awareness = new AwarenessWasm("123", 30_000);
    awareness.setLocalState({ foo: "bar" });
    expect(awareness.getState("123")).toEqual({ foo: "bar" });
    expect(awareness.getAllStates()).toEqual({ "123": { foo: "bar" } });
  });

  it("sync", () => {
    const awareness = new AwarenessWasm("123", 30_000);
    awareness.setLocalState({ foo: "bar" });

    const awarenessB = new AwarenessWasm("223", 30_000);
    const changed = awarenessB.apply(awareness.encode(["123"]));

    expect(changed).toStrictEqual({ added: ["123"], updated: [] });
    expect(awarenessB.getState("123")).toEqual({ foo: "bar" });
    expect(awarenessB.getAllStates()).toEqual({ "123": { foo: "bar" } });
  });

  it("not sync if peer is not in sync list", () => {
    const awareness = new AwarenessWasm("123", 30_000);
    awareness.setLocalState({ foo: "bar" });

    const awarenessB = new AwarenessWasm("223", 30_000);
    awarenessB.apply(awareness.encode(["123"]));
    awarenessB.setLocalState({ new: "bee" });

    const awarenessC = new AwarenessWasm("323", 30_000);
    const changed = awarenessC.apply(awarenessB.encode(["223"]));
    expect(changed).toStrictEqual({ added: ["223"], updated: [] });

    expect(awarenessC.getState("223")).toEqual({ new: "bee" });
    expect(awarenessC.getAllStates()).toEqual({ "223": { new: "bee" } });
  });

  it("should remove outdated", async () => {
    setDebug();
    const awareness = new AwarenessWasm("123", 5);
    awareness.setLocalState({ foo: "bar" });
    await new Promise((r) => setTimeout(r, 10));
    const outdated = awareness.removeOutdated();
    expect(outdated).toEqual(["123"]);
    expect(awareness.getAllStates()).toEqual({});
  });

  it("wrapped", async () => {
    const awareness = new Awareness("1", 10);
    let i = 0;
    const listener = ((arg, origin) => {
      if (i === 0) {
        expect(origin).toBe("local");
        expect(arg).toStrictEqual({
          removed: [],
          updated: [],
          added: ["1"],
        });
      }
      if (i === 1) {
        expect(origin).toBe("remote");
        expect(arg).toStrictEqual({
          removed: [],
          updated: [],
          added: ["2"],
        });
      }
      if (i >= 2) {
        expect(origin).toBe("timeout");
        for (const r of arg.removed) {
          expect(["1", "2"]).toContain(r);
        }
      }

      i += 1;
    }) as AwarenessListener;
    awareness.addListener(listener);
    awareness.setLocalState("123");
    const b = new Awareness("2", 10);
    b.setLocalState("223");
    const bytes = b.encode(["2"]);
    awareness.apply(bytes);
    expect(awareness.getAllStates()).toEqual({ "1": "123", "2": "223" });
    await new Promise((r) => setTimeout(r, 20));
    expect(awareness.getAllStates()).toEqual({});
    expect(i).toBeGreaterThanOrEqual(3);
  });

  it("consistency", () => {
    const a = new AwarenessWasm("1", 10);
    const b = new AwarenessWasm("2", 10);
    a.setLocalState(0);
    const oldBytes = a.encode(["1"]);
    a.setLocalState(1);
    const newBytes = a.encode(["1"]);
    b.apply(newBytes);
    b.apply(oldBytes);
    expect(a.getState("1")).toBe(1);
    expect(b.getState("1")).toBe(1);
    expect(b.peers()).toStrictEqual(["1"]);
    b.setLocalState(2);
    expect(b.peers()).toStrictEqual(["1", "2"]);
  });
});
