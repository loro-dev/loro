import { describe, expect, expectTypeOf, it } from "vitest";
import {
  Delta,
  getType,
  ListDiff,
  Loro,
  LoroEventBatch,
  LoroList,
  LoroMap,
  LoroText,
  MapDiff,
  TextDiff,
} from "../src";

describe("event", () => {
  it("target", async () => {
    const loro = new Loro();
    let lastEvent: undefined | LoroEventBatch;
    loro.subscribe((event) => {
      lastEvent = event;
    });
    const text = loro.getText("text");
    const id = text.id;
    text.insert(0, "123");
    loro.commit();
    await oneMs();
    expect(lastEvent?.events[0].target).toEqual(id);
  });

  it("path", async () => {
    const loro = new Loro();
    let lastEvent: undefined | LoroEventBatch;
    loro.subscribe((event) => {
      lastEvent = event;
    });
    const map = loro.getMap("map");
    const subMap = map.setContainer("sub", new LoroMap());
    subMap.set("0", "1");
    loro.commit();
    await oneMs();
    expect(lastEvent?.events[1].path).toStrictEqual(["map", "sub"]);
    const list = subMap.setContainer("list", new LoroList());
    list.insert(0, "2");
    const text = list.insertContainer(1, new LoroText());
    loro.commit();
    await oneMs();
    text.insert(0, "3");
    loro.commit();
    await oneMs();
    expect(lastEvent?.events[0].path).toStrictEqual(["map", "sub", "list", 1]);
  });

  it("text diff", async () => {
    const loro = new Loro();
    let lastEvent: undefined | LoroEventBatch;
    loro.subscribe((event) => {
      lastEvent = event;
    });
    const text = loro.getText("t");
    text.insert(0, "3");
    loro.commit();
    await oneMs();
    expect(lastEvent?.events[0].diff).toStrictEqual({
      type: "text",
      diff: [{ insert: "3" }],
    } as TextDiff);
    text.insert(1, "12");
    loro.commit();
    await oneMs();
    expect(lastEvent?.events[0].diff).toStrictEqual({
      type: "text",
      diff: [{ retain: 1 }, { insert: "12" }],
    } as TextDiff);
  });

  it("list diff", async () => {
    const loro = new Loro();
    let lastEvent: undefined | LoroEventBatch;
    loro.subscribe((event) => {
      lastEvent = event;
    });
    const text = loro.getList("l");
    text.insert(0, "3");
    loro.commit();
    await oneMs();
    expect(lastEvent?.events[0].diff).toStrictEqual({
      type: "list",
      diff: [{ insert: ["3"] }],
    } as ListDiff);
    text.insert(1, "12");
    loro.commit();
    await oneMs();
    expect(lastEvent?.events[0].diff).toStrictEqual({
      type: "list",
      diff: [{ retain: 1 }, { insert: ["12"] }],
    } as ListDiff);
  });

  it("map diff", async () => {
    const loro = new Loro();
    let lastEvent: undefined | LoroEventBatch;
    loro.subscribe((event) => {
      lastEvent = event;
    });
    const map = loro.getMap("m");
    map.set("0", "3");
    map.set("1", "2");
    loro.commit();
    await oneMs();
    expect(lastEvent?.events[0].diff).toStrictEqual({
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
    expect(lastEvent?.events[0].diff).toStrictEqual({
      type: "map",
      updated: {
        "0": "0",
        "1": "1",
      },
    } as MapDiff);
  });

  it("tree", async () => {
    const loro = new Loro();
    let lastEvent: undefined | LoroEventBatch;
    loro.subscribe((event) => {
      console.log(event);
      lastEvent = event;
    });
    const tree = loro.getTree("tree");
    const id = tree.id;
    tree.createNode();
    loro.commit();
    await oneMs();
    expect(lastEvent?.events[0].target).toEqual(id);
  });

  describe("subscribe container events", () => {
    it("text", async () => {
      const loro = new Loro();
      const text = loro.getText("text");
      let ran = 0;
      const sub = text.subscribe(loro, (event) => {
        if (!ran) {
          expect((event.events[0].diff as any).diff).toStrictEqual([
            { insert: "123" },
          ] as Delta<string>[]);
        }
        ran += 1;
        for (const containerDiff of event.events) {
          expect(containerDiff.target).toBe(text.id);
        }
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

      const subMap = map.setContainer("sub", new LoroMap());
      loro.commit();
      await oneMs();
      expect(times).toBe(1);
      const text = subMap.setContainer("k", new LoroText());
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

      const text = list.insertContainer(0, new LoroText());
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
        for (const containerDiff of event.events) {
          const diff = containerDiff.diff;
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
        if (first) {
          const diff = (e.events[0].diff as ListDiff).diff;
          const text = diff[0].insert![0] as LoroText;
          text.insert(0, "abc");
          first = false;
        }
      });
      list.insertContainer(0, new LoroText());
      loro.commit();
      await oneMs();
      expect(loro.toJson().list[0]).toBe("abc");
    });
  });

  it("diff can contain containers", async () => {
    const doc = new Loro();
    const list = doc.getList("list");
    let ran = false;
    doc.subscribe((event) => {
      if (event.events[0].diff.type === "list") {
        for (const item of event.events[0].diff.diff) {
          const t = item.insert![0] as LoroText;
          expect(t.toString()).toBe("Hello");
          expect(item.insert?.length).toBe(2);
          expect(getType(item.insert![0])).toBe("Text");
          expect(getType(item.insert![1])).toBe("Map");
        }
        ran = true;
      }
    });

    list.insertContainer(0, new LoroMap());
    const t = list.insertContainer(0, new LoroText());
    t.insert(0, "He");
    t.insert(2, "llo");
    doc.commit();
    await new Promise((resolve) => setTimeout(resolve, 1));
    expect(ran).toBeTruthy();
  });
});

function oneMs(): Promise<void> {
  return new Promise((r) => setTimeout(r));
}
