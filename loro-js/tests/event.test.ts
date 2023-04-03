import { describe, expect, it } from "vitest";
import {
  Delta,
  ListDiff,
  Loro,
  LoroEvent,
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

  describe("subscribe container events", () => {
    it("text", async () => {
      const loro = new Loro();
      const text = loro.getText("text");
      let ran = 0;
      let oneTimeRan = 0;
      text.subscribeOnce(loro, (_) => {
        oneTimeRan += 1;
      });
      const sub = text.subscribe(loro, (event) => {
        if (!ran) {
          expect(event.diff[0].diff).toStrictEqual(
            [{ type: "insert", "value": "123" }] as Delta<string>[],
          );
        }
        ran += 1;
        expect(event.target).toBe(text.id);
      });
      text.insert(loro, 0, "123");
      text.insert(loro, 1, "456");
      await zeroMs();
      expect(ran).toBeTruthy();
      // subscribeOnce test
      expect(oneTimeRan).toBe(1);
      expect(text.toString()).toEqual("145623");

      // unsubscribe
      const oldRan = ran;
      text.unsubscribe(loro, sub);
      text.insert(loro, 0, "789");
      expect(ran).toBe(oldRan);
    });

    it("map subscribe deep", async () => {
      const loro = new Loro();
      const map = loro.getMap("map");
      let times = 0;
      const sub = map.subscribeDeep(loro, (event) => {
        times += 1;
      });

      const subMap = map.insertContainer(loro, "sub", "Map");
      await zeroMs();
      expect(times).toBe(1);
      const text = subMap.insertContainer(loro, "k", "Text");
      await zeroMs();
      expect(times).toBe(2);
      text.insert(loro, 0, "123");
      await zeroMs();
      expect(times).toBe(3);

      // unsubscribe
      map.unsubscribe(loro, sub);
      text.insert(loro, 0, "123");
      await zeroMs();
      expect(times).toBe(3);
    });

    it("list subscribe deep", async () => {
      const loro = new Loro();
      const list = loro.getList("list");
      let times = 0;
      const sub = list.subscribeDeep(loro, (_) => {
        times += 1;
      });

      const text = list.insertContainer(loro, 0, "Text");
      await zeroMs();
      expect(times).toBe(1);
      text.insert(loro, 0, "123");
      await zeroMs();
      expect(times).toBe(2);

      // unsubscribe
      list.unsubscribe(loro, sub);
      text.insert(loro, 0, "123");
      await zeroMs();
      expect(times).toBe(2);
    });
  });

  describe("text event length should be utf16", () => {
    it("test", async () => {
      const loro = new Loro();
      const text = loro.getText("text");
      let string = "";
      text.subscribe(loro, (event) => {
        for (const diff of event.diff) {
          expect(diff.type).toBe("text");
          if (diff.type === "text") {
            let newString = "";
            let pos = 0;
            for (const delta of diff.diff) {
              if (delta.type === "retain") {
                pos += delta.len;
                newString += string.slice(0, pos);
              } else if (delta.type === "insert") {
                newString += delta.value;
              } else {
                pos += delta.len;
              }
            }

            string = newString + string.slice(pos);
          }
        }
      });
      text.insert(loro, 0, "你好");
      await zeroMs();
      expect(text.toString()).toBe(string);

      text.insert(loro, 1, "世界");
      await zeroMs();
      expect(text.toString()).toBe(string);

      text.insert(loro, 2, "👍");
      await zeroMs();
      expect(text.toString()).toBe(string);

      text.insert(loro, 4, "♪(^∇^*)");
      await zeroMs();
      expect(text.toString()).toBe(string);
    });
  });
});

function zeroMs(): Promise<void> {
  return new Promise((r) => setTimeout(r));
}
