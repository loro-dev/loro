import { describe, expect, expectTypeOf, it } from "vitest";
import {
  Delta,
  ListDiff,
  Loro,
  LoroText,
  LoroEvent,
  MapDiff,
  TextDiff,
  setPanicHook,
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
    text.insert(0, "123");
    loro.commit();
    await oneMs();
    expect(lastEvent?.target).toEqual(id);
  });

  it("event id", async () => {
    const loro = new Loro();
    let lastEventId: bigint = 0n;
    let uniqueEvent = 0;
    loro.subscribe((event) => {
      console.log(event.id);
      if (event.id === lastEventId) {
        return;
      }

      lastEventId = event.id;
      uniqueEvent += 1;
    });

    const map = loro.getMap("map");
    map.set("0", 100);
    loro.commit();
    await oneMs();
    expect(uniqueEvent).toBe(1);
    const bDoc = new Loro();
    bDoc.getText("text").insert(0, "1");
    bDoc.getText("text").insert(1, "1");
    bDoc.commit();
    await oneMs();
    bDoc.getMap("map").set("0", "1");
    bDoc.commit();
    await oneMs();
    loro.import(bDoc.exportFrom());
    await oneMs();
    expect(uniqueEvent).toBe(2);
  });

  it("path", async () => {
    const loro = new Loro();
    let lastEvent: undefined | LoroEvent;
    loro.subscribe((event) => {
      lastEvent = event;
    });
    const map = loro.getMap("map");
    const subMap = map.setContainer("sub", "Map");
    subMap.set("0", "1");
    loro.commit();
    await oneMs();

    expect(lastEvent?.path).toStrictEqual(["map", "sub"]);
    const list = subMap.setContainer("list", "List");
    list.insert(0, "2");
    const text = list.insertContainer(1, "Text");
    loro.commit();
    await oneMs();
    text.insert(0, "3");
    loro.commit();
    await oneMs();
    expect(lastEvent?.path).toStrictEqual(["map", "sub", "list", 1]);
  });

  it("text diff", async () => {
    const loro = new Loro();
    let lastEvent: undefined | LoroEvent;
    loro.subscribe((event) => {
      lastEvent = event;
    });
    const text = loro.getText("t");
    text.insert(0, "3");
    loro.commit();
    await oneMs();
    expect(lastEvent?.diff).toStrictEqual({
      type: "text",
      diff: [{ insert: "3" }],
    } as TextDiff);
    text.insert(1, "12");
    loro.commit();
    await oneMs();
    expect(lastEvent?.diff).toStrictEqual({
      type: "text",
      diff: [{ retain: 1 }, { insert: "12" }],
    } as TextDiff);
  });

  it("list diff", async () => {
    const loro = new Loro();
    let lastEvent: undefined | LoroEvent;
    loro.subscribe((event) => {
      lastEvent = event;
    });
    const text = loro.getList("l");
    text.insert(0, "3");
    loro.commit();
    await oneMs();
    expect(lastEvent?.diff).toStrictEqual({
      type: "list",
      diff: [{ insert: ["3"] }],
    } as ListDiff);
    text.insert(1, "12");
    loro.commit();
    await oneMs();
    expect(lastEvent?.diff).toStrictEqual({
      type: "list",
      diff: [{ retain: 1 }, { insert: ["12"] }],
    } as ListDiff);
  });

  it("map diff", async () => {
    const loro = new Loro();
    let lastEvent: undefined | LoroEvent;
    loro.subscribe((event) => {
      lastEvent = event;
    });
    const map = loro.getMap("m");
    map.set("0", "3");
    map.set("1", "2");
    loro.commit();
    await oneMs();
    expect(lastEvent?.diff).toStrictEqual({
      type: "map",
      updated: {
        "0": "3",
        "1": "2",
      },
    } as MapDiff);
    map.set("0", "0");
    map.set("1", "1");
    loro.commit();
    await oneMs();
    expect(lastEvent?.diff).toStrictEqual({
      type: "map",
      updated: {
        "0": "0",
        "1": "1",
      },
    } as MapDiff);
  });

  describe("subscribe container events", () => {
    it("text", async () => {
      const loro = new Loro();
      const text = loro.getText("text");
      let ran = 0;
      const sub = text.subscribe(loro, (event) => {
        if (!ran) {
          expect((event.diff as any).diff).toStrictEqual([
            { insert: "123" },
          ] as Delta<string>[]);
        }
        ran += 1;
        expect(event.target).toBe(text.id);
      });

      text.insert(0, "123");
      loro.commit();
      await oneMs();
      text.insert(1, "456");
      loro.commit();
      await oneMs();
      expect(ran).toBeTruthy();
      // subscribeOnce test
      expect(text.toString()).toEqual("145623");

      // unsubscribe
      const oldRan = ran;
      text.unsubscribe(loro, sub);
      text.insert(0, "789");
      loro.commit();
      await oneMs();
      expect(ran).toBe(oldRan);
    });

    it("map subscribe deep", async () => {
      const loro = new Loro();
      const map = loro.getMap("map");
      let times = 0;
      const sub = map.subscribe(loro, (event) => {
        times += 1;
      });

      const subMap = map.setContainer("sub", "Map");
      loro.commit();
      await oneMs();
      expect(times).toBe(1);
      const text = subMap.setContainer("k", "Text");
      loro.commit();
      await oneMs();
      expect(times).toBe(2);
      text.insert(0, "123");
      loro.commit();
      await oneMs();
      expect(times).toBe(3);

      // unsubscribe
      loro.unsubscribe(sub);
      text.insert(0, "123");
      loro.commit();
      await oneMs();
      expect(times).toBe(3);
    });

    it("list subscribe deep", async () => {
      const loro = new Loro();
      const list = loro.getList("list");
      let times = 0;
      const sub = list.subscribe(loro, (_) => {
        times += 1;
      });

      const text = list.insertContainer(0, "Text");
      loro.commit();
      await oneMs();
      expect(times).toBe(1);
      text.insert(0, "123");
      await oneMs();
      loro.commit();
      await oneMs();
      expect(times).toBe(2);

      // unsubscribe
      loro.unsubscribe(sub);
      text.insert(0, "123");
      loro.commit();
      await oneMs();
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
            if (delta.retain != null) {
              newString += string.slice(pos, pos + delta.retain);
              pos += delta.retain;
            } else if (delta.insert != null) {
              newString += delta.insert;
            } else {
              pos += delta.delete;
            }
          }

          string = newString + string.slice(pos);
        }
      });
      text.insert(0, "你好");
      loro.commit();
      await oneMs();
      expect(text.toString()).toBe(string);

      text.insert(1, "世界");
      loro.commit();
      await oneMs();
      expect(text.toString()).toBe(string);

      text.insert(2, "👍");
      loro.commit();
      await oneMs();
      expect(text.toString()).toBe(string);

      text.insert(2, "♪(^∇^*)");
      loro.commit();
      await oneMs();
      expect(text.toString()).toBe(string);
    });
  });

  describe("handler in event", () => {
    it("test", async () => {
      const loro = new Loro();
      const list = loro.getList("list");
      let first = true;
      loro.subscribe((e) => {
        if(first){
          const diff = e.diff.diff;
          const text = diff[0].insert[0] as LoroText;
          text.insert(0, "abc");
          first = false;
        }
      });
      list.insertContainer(0, "Text");
      loro.commit();
      await oneMs();
      expect(loro.toJson().list[0]).toBe('abc');
    });
  });
});

function oneMs(): Promise<void> {
  return new Promise((r) => setTimeout(r));
}
