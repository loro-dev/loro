import { describe, expect, it } from "vitest";
import {
  Delta,
  getType,
  ListDiff,
  Loro,
  LoroDoc,
  LoroEventBatch,
  LoroList,
  LoroMap,
  LoroText,
  MapDiff,
  TextDiff,
} from "../src";

describe("event", () => {
  it("target", async () => {
    const loro = new LoroDoc();
    let lastEvent: undefined | LoroEventBatch;
    loro.subscribe((event) => {
      expect(event.by).toBe("local");
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
    const loro = new LoroDoc();
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
    const loro = new LoroDoc();
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
    const loro = new LoroDoc();
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
    const loro = new LoroDoc();
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
    const loro = new LoroDoc();
    let lastEvent: undefined | LoroEventBatch;
    loro.subscribe((event) => {
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
      const loro = new LoroDoc();
      const text = loro.getText("text");
      let ran = 0;
      const sub = text.subscribe((event) => {
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
      text.unsubscribe(sub);
      text.insert(0, "789");
      loro.commit();
      await oneMs();
      expect(ran).toBe(oldRan);
    });

    it("map subscribe deep", async () => {
      const loro = new LoroDoc();
      const map = loro.getMap("map");
      let times = 0;
      const sub = map.subscribe((event) => {
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
      const loro = new LoroDoc();
      const list = loro.getList("list");
      let times = 0;
      const sub = list.subscribe((event) => {
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
      const loro = new LoroDoc();
      const text = loro.getText("text");
      let string = "";
      text.subscribe((event) => {
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
      const loro = new LoroDoc();
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
      expect(loro.toJSON().list[0]).toBe("abc");
    });
  });

  it("diff can contain containers", async () => {
    const doc = new LoroDoc();
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

  it("remote event", async () => {
    const doc = new LoroDoc();
    const list = doc.getList("list");
    list.insert(0, 123);
    {
      const doc2 = new LoroDoc();
      let triggered = false;
      doc2.subscribe((event) => {
        expect(event.by).toBe("import");
        triggered = true;
      });
      doc2.import(doc.exportFrom());
      await oneMs();
      expect(triggered).toBeTruthy();
    }
    {
      const doc2 = new LoroDoc();
      let triggered = false;
      doc2.subscribe((event) => {
        expect(event.by).toBe("import");
        triggered = true;
      });
      doc2.import(doc.exportSnapshot());
      await oneMs();
      expect(triggered).toBeTruthy();
    }
  });

  it("checkout event", async () => {
    const doc = new LoroDoc();
    const list = doc.getList("list");
    list.insert(0, 123);
    doc.commit();
    let triggered = false;
    doc.subscribe((e) => {
      expect(e.by).toBe("checkout");
      triggered = true;
    });

    doc.checkout([]);
    await oneMs();
    expect(triggered).toBeTruthy();
  });

  describe("local updates events", () => {
    it("basic", () => {
      const loro = new Loro();
      const text = loro.getText("text");
      let updateReceived = false;

      const unsubscribe = loro.subscribeLocalUpdates((update) => {
        updateReceived = true;
        expect(update).toBeInstanceOf(Uint8Array);
        expect(update.length).toBeGreaterThan(0);
      });

      text.insert(0, "Hello");
      loro.commit();

      expect(updateReceived).toBe(true);

      // Test unsubscribe
      updateReceived = false;
      unsubscribe();

      text.insert(5, " World");
      loro.commit();

      expect(updateReceived).toBe(false);
    });

    it("multiple subscribers", () => {
      const loro = new Loro();
      const text = loro.getText("text");
      let count1 = 0;
      let count2 = 0;

      const unsubscribe1 = loro.subscribeLocalUpdates(() => {
        count1++;
      });

      const unsubscribe2 = loro.subscribeLocalUpdates(() => {
        count2++;
      });

      text.insert(0, "Hello");
      loro.commit();

      expect(count1).toBe(1);
      expect(count2).toBe(1);

      unsubscribe1();

      text.insert(5, " World");
      loro.commit();

      expect(count1).toBe(1);
      expect(count2).toBe(2);

      unsubscribe2();
    });

    it("updates for different containers", () => {
      const loro = new Loro();
      const text = loro.getText("text");
      const list = loro.getList("list");
      const map = loro.getMap("map");
      let updates = 0;

      loro.subscribeLocalUpdates(() => {
        updates++;
      });

      text.insert(0, "Hello");
      list.push("World");
      map.set("key", "value");
      loro.commit();

      expect(updates).toBe(1);  // All changes are bundled in one update

      text.insert(5, "!");
      loro.commit();

      expect(updates).toBe(2);
    })

    it("can be used to sync", () => {
      const loro1 = new Loro();
      const loro2 = new Loro();
      const text1 = loro1.getText("text");
      const text2 = loro2.getText("text");

      loro1.subscribeLocalUpdates((updates) => {
        loro2.import(updates);
      });

      loro2.subscribeLocalUpdates((updates) => {
        loro1.import(updates);
      });

      text1.insert(0, "Hello");
      loro1.commit();

      expect(text2.toString()).toBe("Hello");

      text2.insert(5, " World");
      loro2.commit();

      expect(text1.toString()).toBe("Hello World");

      // Test concurrent edits
      text1.insert(0, "1. ");
      text2.insert(text2.length, "!");
      loro1.commit();
      loro2.commit();

      // Both documents should converge to the same state
      expect(text1.toString()).toBe("1. Hello World!");
      expect(text2.toString()).toBe("1. Hello World!");
    })
  })
});

function oneMs(): Promise<void> {
  return new Promise((r) => setTimeout(r));
}
