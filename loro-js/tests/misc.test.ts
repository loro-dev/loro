import { describe, expect, it } from "vitest";
import {
  Delta,
  ListDiff,
  Loro,
  LoroEvent,
  LoroMap,
  MapDIff as MapDiff,
  PrelimList,
  PrelimMap,
  PrelimText,
  TextDiff,
  Transaction,
} from "../src";

function assertEquals(a: any, b: any) {
  expect(a).toStrictEqual(b);
}

describe("transaction", () => {
  it("transaction", async () => {
    const loro = new Loro();
    const text = loro.getText("text");
    let count = 0;
    const sub = loro.subscribe(() => {
      count += 1;
      loro.unsubscribe(sub);
    });
    loro.transact((txn: Transaction) => {
      expect(count).toBe(0);
      text.insert(txn, 0, "hello world");
      expect(count).toBe(0);
      text.insert(txn, 0, "hello world");
      assertEquals(count, 0);
    });
    await one_ms();
    assertEquals(count, 1);
  });

  it("transaction origin", async () => {
    const loro = new Loro();
    const text = loro.getText("text");
    let count = 0;
    const sub = loro.subscribe((event: { origin: string }) => {
      count += 1;
      loro.unsubscribe(sub);
      assertEquals(event.origin, "origin");
    });
    loro.transact((txn: Transaction) => {
      assertEquals(count, 0);
      text.insert(txn, 0, "hello world");
      assertEquals(count, 0);
      text.insert(txn, 0, "hello world");
      assertEquals(count, 0);
    }, "origin");
    await one_ms();
    assertEquals(count, 1);
  });
});

describe("subscribe", () => {
  it("subscribe_lock", async () => {
    const loro = new Loro();
    const text = loro.getText("text");
    const list = loro.getList("list");
    let count = 0;
    let i = 1;
    const sub = loro.subscribe(() => {
      if (i > 0) {
        list.insert(loro, 0, i);
        i--;
      }
      count += 1;
    });
    text.insert(loro, 0, "hello world");
    await one_ms();
    assertEquals(count, 2);
    text.insert(loro, 0, "hello world");
    await one_ms();
    assertEquals(count, 3);
    loro.unsubscribe(sub);
    text.insert(loro, 0, "hello world");
    await one_ms();
    assertEquals(count, 3);
  });

  it("subscribe_lock2", async () => {
    const loro = new Loro();
    const text = loro.getText("text");
    let count = 0;
    const sub = loro.subscribe(() => {
      count += 1;
      loro.unsubscribe(sub);
    });
    assertEquals(count, 0);
    text.insert(loro, 0, "hello world");
    await one_ms();
    assertEquals(count, 1);
    text.insert(loro, 0, "hello world");
    assertEquals(count, 1);
  });

  it("subscribe", async () => {
    const loro = new Loro();
    const text = loro.getText("text");
    let count = 0;
    const sub = loro.subscribe(() => {
      count += 1;
    });
    text.insert(loro, 0, "hello world");
    await one_ms();
    assertEquals(count, 1);
    text.insert(loro, 0, "hello world");
    await one_ms();
    assertEquals(count, 2);
    loro.unsubscribe(sub);
    text.insert(loro, 0, "hello world");
    await one_ms();
    assertEquals(count, 2);
  });
});

describe("sync", () => {
  it("two insert at beginning", async () => {
    const a = new Loro();
    const b = new Loro();
    let a_version: undefined | Uint8Array = undefined;
    let b_version: undefined | Uint8Array = undefined;
    a.subscribe((e: { local: boolean }) => {
      if (e.local) {
        const exported = a.exportFrom(a_version);
        b.import(exported);
        a_version = a.version();
      }
    });
    b.subscribe((e: { local: boolean }) => {
      if (e.local) {
        const exported = b.exportFrom(b_version);
        a.import(exported);
        b_version = b.version();
      }
    });
    const aText = a.getText("text");
    const bText = b.getText("text");
    aText.insert(a, 0, "abc");
    await one_ms();
    assertEquals(aText.toString(), bText.toString());
  });

  it("sync", () => {
    const loro = new Loro();
    const text = loro.getText("text");
    text.insert(loro, 0, "hello world");
    const loro_bk = new Loro();
    loro_bk.import(loro.exportFrom(undefined));
    assertEquals(loro_bk.toJson(), loro.toJson());
    const text_bk = loro_bk.getText("text");
    assertEquals(text_bk.toString(), "hello world");
    text_bk.insert(loro_bk, 0, "a ");
    loro.import(loro_bk.exportFrom(undefined));
    assertEquals(text.toString(), "a hello world");
    const map = loro.getMap("map");
    map.set(loro, "key", "value");
  });
});

describe("prelim", () => {
  it("test prelim", async (t) => {
    const loro = new Loro();
    const map = loro.getMap("map");
    const list = loro.getList("list");
    const prelim_text = new PrelimText(undefined);
    const prelim_map = new PrelimMap({ a: 1, b: 2 });
    const prelim_list = new PrelimList([1, "2", { a: 4 }]);

    it("prelim text", () => {
      prelim_text.insert(0, "hello world");
      assertEquals(prelim_text.value, "hello world");
      prelim_text.delete(6, 5);
      prelim_text.insert(6, "everyone");
      assertEquals(prelim_text.value, "hello everyone");
    });

    it("prelim map", () => {
      prelim_map.set("ab", 123);
      assertEquals(prelim_map.value, { a: 1, b: 2, ab: 123 });
      prelim_map.delete("b");
      assertEquals(prelim_map.value, { a: 1, ab: 123 });
    });

    it("prelim list", () => {
      prelim_list.insert(0, 0);
      assertEquals(prelim_list.value, [0, 1, "2", { a: 4 }]);
      prelim_list.delete(1, 2);
      assertEquals(prelim_list.value, [0, { a: 4 }]);
    });

    it("prelim map integrate", () => {
      map.set(loro, "text", prelim_text);
      map.set(loro, "map", prelim_map);
      map.set(loro, "list", prelim_list);
      assertEquals(map.getValueDeep(loro), {
        text: "hello everyone",
        map: { a: 1, ab: 123 },
        list: [0, { a: 4 }],
      });
    });

    it("prelim list integrate", () => {
      const prelim_text = new PrelimText("ttt");
      const prelim_map = new PrelimMap({ a: 1, b: 2 });
      const prelim_list = new PrelimList([1, "2", { a: 4 }]);
      list.insert(loro, 0, prelim_text);
      list.insert(loro, 1, prelim_map);
      list.insert(loro, 2, prelim_list);
      assertEquals(list.getValueDeep(loro), ["ttt", { a: 1, b: 2 }, [1, "2", {
        a: 4,
      }]]);
    });
  });
});

describe("wasm", () => {
  const loro = new Loro();
  const a = loro.getText("ha");
  a.insert(loro, 0, "hello world");
  a.delete(loro, 6, 5);
  a.insert(loro, 6, "everyone");
  const b = loro.getMap("ha");
  b.set(loro, "ab", 123);

  const bText = b.insertContainer(loro, "hh", "Text");

  it("map get", () => {
    assertEquals(b.get("ab"), 123);
  });

  it("getValueDeep", () => {
    bText.insert(loro, 0, "hello world Text");
    assertEquals(b.getValueDeep(loro), { ab: 123, hh: "hello world Text" });
  });

  it("should throw error when using the wrong context", () => {
    expect(() => {
      const loro2 = new Loro();
      bText.insert(loro2, 0, "hello world Text");
    }).toThrow();
  });

  it("get container by id", () => {
    const id = b.id;
    const b2 = loro.getContainerById(id) as LoroMap;
    assertEquals(b2.value, b.value);
    assertEquals(b2.id, id);
    b2.set(loro, "0", 12);
    assertEquals(b2.value, b.value);
  });
});

function one_ms(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 1));
}