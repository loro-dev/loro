import { describe, expect, it } from "vitest";
import {
  Delta,
  ListDiff,
  Loro,
  LoroEvent,
  MapDiff as MapDiff,
  TextDiff,
  setPanicHook
} from "../src";

setPanicHook();
describe("event", () => {
  it("target", async () => {
    const loro = new Loro();
    let lastEvent: undefined | LoroEvent;
    loro.subscribe((event) => {
      lastEvent = event;
    });
    const text = loro.getText("text");
    const id = text.id;
    loro.transact((tx) => {
      text.insert(tx, 0, "123");
    });
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
    const subMap = loro.transact((tx) => {
      const subMap = map.insertContainer(tx, "sub", "Map");
      subMap.set(tx, "0", "1");
      return subMap;
    });

    await zeroMs();
    expect(lastEvent?.path).toStrictEqual(["map", "sub"]);
    const text = loro.transact((tx) => {
      const list = subMap.insertContainer(tx, "list", "List");
      list.insert(tx, 0, "2");
      const text = list.insertContainer(tx, 1, "Text");
      return text;
    });
    await zeroMs();
    loro.transact((tx) => {
      text.insert(tx, 0, "3");
    });
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
    loro.transact(tx => {
      text.insert(tx, 0, "3");
    })
    await zeroMs();
    expect(lastEvent?.diff).toStrictEqual(
      { type: "text", diff: [{ type: "insert", value: "3" }] } as TextDiff,
    );
    loro.transact(tx => {
      text.insert(tx, 1, "12");
    })
    await zeroMs();
    expect(lastEvent?.diff).toStrictEqual(
      {
        type: "text",
        diff: [{ type: "retain", len: 1 }, { type: "insert", value: "12" }],
      } as TextDiff,
    );
  });

  it("list diff", async () => {
    const loro = new Loro();
    let lastEvent: undefined | LoroEvent;
    loro.subscribe((event) => {
      lastEvent = event;
    });
    const text = loro.getList("l");
    loro.transact(tx => {
      text.insert(tx, 0, "3");
    })
    await zeroMs();
    expect(lastEvent?.diff).toStrictEqual(
      { type: "list", diff: [{ type: "insert", value: ["3"] }] } as ListDiff,
    );
    loro.transact(tx => {
      text.insert(tx, 1, "12");
    })
    await zeroMs();
    expect(lastEvent?.diff).toStrictEqual(
      {
        type: "list",
        diff: [{ type: "retain", len: 1 }, { type: "insert", value: ["12"] }],
      } as ListDiff,
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
      {
        type: "map",
        updated: {
          "0": "3",
          "1": "2",
        }
      } as MapDiff,
    );
    loro.transact((tx) => {
      map.set(tx, "0", "0");
      map.set(tx, "1", "1");
    });
    await zeroMs();
    expect(lastEvent?.diff).toStrictEqual(
      {
        type: "map",
        updated: {
          "0": "0",
          "1": "1"
        },
      } as MapDiff,
    );
  });

  describe("subscribe container events", () => {
    it("text", async () => {
      const loro = new Loro();
      const text = loro.getText("text");
      let ran = 0;
      const sub = text.subscribe(loro, (event) => {
        if (!ran) {
          expect(event.diff.diff).toStrictEqual(
            [{ type: "insert", "value": "123" }] as Delta<string>[],
          );
        }
        ran += 1;
        expect(event.target).toBe(text.id);
      });

      loro.transact(tx => {
        text.insert(tx, 0, "123");
      });
      loro.transact(tx => {
        text.insert(tx, 1, "456");
      });
      await zeroMs();
      expect(ran).toBeTruthy();
      // subscribeOnce test
      expect(text.toString()).toEqual("145623");

      // unsubscribe
      const oldRan = ran;
      text.unsubscribe(loro, sub);
      loro.transact(tx => {
        text.insert(tx, 0, "789");
      })
      expect(ran).toBe(oldRan);
    });

    it("map subscribe deep", async () => {
      const loro = new Loro();
      const map = loro.getMap("map");
      let times = 0;
      const sub = map.subscribe(loro, (event) => {
        times += 1;
      });


      const subMap =
        loro.transact(tx =>
          map.insertContainer(tx, "sub", "Map")
        );
      await zeroMs();
      expect(times).toBe(1);
      const text = loro.transact(tx => subMap.insertContainer(tx, "k", "Text"));
      await zeroMs();
      expect(times).toBe(2);
      loro.transact(tx => text.insert(tx, 0, "123"));
      await zeroMs();
      expect(times).toBe(3);

      // unsubscribe
      loro.unsubscribe(sub);
      loro.transact(tx => text.insert(tx, 0, "123"));
      await zeroMs();
      expect(times).toBe(3);
    });

    it("list subscribe deep", async () => {
      const loro = new Loro();
      const list = loro.getList("list");
      let times = 0;
      const sub = list.subscribe(loro, (_) => {
        times += 1;
      });

      const text = loro.transact(tx => list.insertContainer(tx, 0, "Text"));
      await zeroMs();
      expect(times).toBe(1);
      loro.transact(tx => text.insert(tx, 0, "123"));
      await zeroMs();
      expect(times).toBe(2);

      // unsubscribe
      loro.unsubscribe(sub);
      loro.transact(tx => text.insert(tx, 0, "123"));
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
        const diff = event.diff;
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
      });
      loro.transact(tx => text.insert(tx, 0, "你好"));
      await zeroMs();
      expect(text.toString()).toBe(string);

      loro.transact(tx => text.insert(tx, 1, "世界"));
      await zeroMs();
      expect(text.toString()).toBe(string);

      loro.transact(tx => text.insert(tx, 2, "👍"));
      await zeroMs();
      expect(text.toString()).toBe(string);

      loro.transact(tx => text.insert(tx, 2, "♪(^∇^*)"));
      await zeroMs();
      expect(text.toString()).toBe(string);
    });
  });
});

function zeroMs(): Promise<void> {
  return new Promise((r) => setTimeout(r));
}
