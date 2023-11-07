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
