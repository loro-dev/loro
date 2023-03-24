import { describe, expect, it } from "vitest";
import { Loro, LoroEvent, LoroMap } from "../src";

describe("event", () => {
  it("target", async () => {
    const loro = new Loro();
    let lastEvent: undefined | LoroEvent;
    loro.subscribe((event) => {
      lastEvent = event;
    });
    const text = loro.getText("text");
    const id = text.id;
    text.insert(loro, 0, "123");
    await zeroMs();
    expect(lastEvent?.target).toEqual(id);
  });

  it("path", async () => {
    const loro = new Loro();
    let lastEvent: undefined | LoroEvent;
    loro.subscribe((event) => {
      lastEvent = event;
    });
    const map = loro.getMap("map");
    const subMap = map.insertContainer(loro, "sub", "Map");
    subMap.set(loro, "0", "1");
    await zeroMs();
    expect(lastEvent?.path).toStrictEqual(["map", "sub"]);
    const list = subMap.insertContainer(loro, "list", "List");
    list.insert(loro, 0, "2");
    const text = list.insertContainer(loro, 1, "Text");
    text.insert(loro, 0, "3");
    await zeroMs();
    expect(lastEvent?.path).toStrictEqual(["map", "sub", "list", 1]);
  });
});

function zeroMs(): Promise<void> {
  return new Promise((r) => setTimeout(r));
}
