import { describe, expect, it } from "vitest";
import {
  Diff,
  ListDiff,
  Loro,
  LoroEvent,
  LoroMap,
  MapDIff as MapDiff,
  TextDiff,
} from "../src";

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
    await zeroMs();
    text.insert(loro, 0, "3");
    await zeroMs();
    expect(lastEvent?.path).toStrictEqual(["map", "sub", "list", 1]);
  });

  it("text diff", async () => {
    const loro = new Loro();
    let lastEvent: undefined | LoroEvent;
    loro.subscribe((event) => {
      lastEvent = event;
    });
    const text = loro.getText("t");
    text.insert(loro, 0, "3");
    await zeroMs();
    expect(lastEvent?.diff).toStrictEqual(
      [{ type: "text", diff: [{ type: "insert", value: "3" }] } as TextDiff],
    );
    text.insert(loro, 1, "12");
    await zeroMs();
    expect(lastEvent?.diff).toStrictEqual(
      [{
        type: "text",
        diff: [{ type: "retain", len: 1 }, { type: "insert", value: "12" }],
      } as TextDiff],
    );
  });

  it("list diff", async () => {
    const loro = new Loro();
    let lastEvent: undefined | LoroEvent;
    loro.subscribe((event) => {
      lastEvent = event;
    });
    const text = loro.getList("l");
    text.insert(loro, 0, "3");
    await zeroMs();
    expect(lastEvent?.diff).toStrictEqual(
      [{ type: "list", diff: [{ type: "insert", value: ["3"] }] } as ListDiff],
    );
    text.insert(loro, 1, "12");
    await zeroMs();
    expect(lastEvent?.diff).toStrictEqual(
      [{
        type: "list",
        diff: [{ type: "retain", len: 1 }, { type: "insert", value: ["12"] }],
      } as ListDiff],
    );
  });

  it("map diff", async () => {
    const loro = new Loro();
    let lastEvent: undefined | LoroEvent;
    loro.subscribe((event) => {
      lastEvent = event;
    });
    const map = loro.getMap("m");
    loro.transact((tx) => {
      map.set(tx, "0", "3");
      map.set(tx, "1", "2");
    });
    await zeroMs();
    expect(lastEvent?.diff).toStrictEqual(
      [{
        type: "map",
        diff: {
          added: {
            "0": "3",
            "1": "2",
          },
          deleted: {},
          updated: {},
        },
      } as MapDiff],
    );
    loro.transact((tx) => {
      map.set(tx, "0", "0");
      map.set(tx, "1", "1");
    });
    await zeroMs();
    expect(lastEvent?.diff).toStrictEqual(
      [{
        type: "map",
        diff: {
          added: {},
          updated: {
            "0": { old: "3", new: "0" },
            "1": { old: "2", new: "1" },
          },
          deleted: {},
        },
      } as MapDiff],
    );
  });
});

function zeroMs(): Promise<void> {
  return new Promise((r) => setTimeout(r));
}
