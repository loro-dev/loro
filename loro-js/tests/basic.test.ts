import { describe, expect, it } from "vitest";
import {
  ContainerID,
  Loro,
  setPanicHook,
} from "../src";

setPanicHook();

describe("list", () => {
  it("insert containers", () => {
    const doc = new Loro();
    const list = doc.getList("list");
    const map = list.insertContainer(0, "Map");
    map.set("key", "value");
    const v = list.get(0);
    console.log(v);
    expect(typeof v).toBe("string");
    const m = doc.getMap(v as ContainerID);
    expect(m.getDeepValue()).toStrictEqual({ key: "value" });
  })

  it.todo("iterate");
})

describe("import", () => {
  it('pending', () => {
    const a = new Loro();
    a.getText("text").insert(0, "a");
    const b = new Loro();
    b.import(a.exportFrom());
    b.getText("text").insert(1, "b");
    const c = new Loro();
    c.import(b.exportFrom());
    c.getText("text").insert(2, "c");

    // c export from b's version, which cannot be imported directly to a. 
    // This operation is pending.
    a.import(c.exportFrom(b.version()))
    expect(a.getText("text").toString()).toBe("a");

    // a import the missing ops from b. It makes the pending operation from c valid.
    a.import(b.exportFrom(a.version()))
    expect(a.getText("text").toString()).toBe("abc");
  })
})
