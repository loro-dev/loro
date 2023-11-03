import { assertType, describe, expect, it } from "vitest";
import {
  Loro,
  LoroList,
  LoroMap,
  PrelimList,
  PrelimMap,
  PrelimText,
  setPanicHook,
} from "../src";
import { expectTypeOf } from "vitest";

setPanicHook();

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
    expect(count).toBe(0);
    text.insert(0, "hello world");
    expect(count).toBe(0);
    text.insert(0, "hello world");
    assertEquals(count, 0);
    loro.commit();
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

    assertEquals(count, 0);
    text.insert(0, "hello world");
    assertEquals(count, 0);
    text.insert(0, "hello world");
    assertEquals(count, 0);
    loro.commit("origin");
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
        list.insert(0, i);
        loro.commit();
        i--;
      }

      count += 1;
    });

    text.insert(0, "hello world");
    loro.commit();

    assertEquals(count, 2);
    text.insert(0, "hello world");
    loro.commit();
    assertEquals(count, 3);
    loro.unsubscribe(sub);
    text.insert(0, "hello world");
    loro.commit();
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
    text.insert(0, "hello world");
    loro.commit();

    assertEquals(count, 1);
    text.insert(0, "hello world");
    loro.commit();

    assertEquals(count, 1);
  });

  it("subscribe", async () => {
    const loro = new Loro();
    const text = loro.getText("text");
    let count = 0;
    const sub = loro.subscribe(() => {
      count += 1;
    });
    text.insert(0, "hello world");
    loro.commit();
    assertEquals(count, 1);
    text.insert(0, "hello world");
    loro.commit();
    assertEquals(count, 2);
    loro.unsubscribe(sub);
    text.insert(0, "hello world");
    loro.commit();
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
    aText.insert(0, "abc");
    a.commit();

    assertEquals(aText.toString(), bText.toString());
  });

  it("sync", () => {
    const loro = new Loro();
    const text = loro.getText("text");
    text.insert(0, "hello world");

    const loro_bk = new Loro();
    loro_bk.import(loro.exportFrom(undefined));
    assertEquals(loro_bk.toJson(), loro.toJson());
    const text_bk = loro_bk.getText("text");
    assertEquals(text_bk.toString(), "hello world");
    text_bk.insert(0, "a ");

    loro.import(loro_bk.exportFrom(undefined));
    assertEquals(text.toString(), "a hello world");
    const map = loro.getMap("map");
    map.set("key", "value");
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
      map.set("text", prelim_text);
      map.set("map", prelim_map);
      map.set("list", prelim_list);
      loro.commit();

      assertEquals(map.getDeepValue(), {
        text: "hello everyone",
        map: { a: 1, ab: 123 },
        list: [0, { a: 4 }],
      });
    });

    it("prelim list integrate", () => {
      const prelim_text = new PrelimText("ttt");
      const prelim_map = new PrelimMap({ a: 1, b: 2 });
      const prelim_list = new PrelimList([1, "2", { a: 4 }]);
      list.insert(0, prelim_text);
      list.insert(1, prelim_map);
      list.insert(2, prelim_list);
      loro.commit();

      assertEquals(list.getDeepValue(), [
        "ttt",
        { a: 1, b: 2 },
        [
          1,
          "2",
          {
            a: 4,
          },
        ],
      ]);
    });
  });
});

describe("wasm", () => {
  const loro = new Loro();
  const a = loro.getText("ha");
  a.insert(0, "hello world");
  a.delete(6, 5);
  a.insert(6, "everyone");
  loro.commit();

  const b = loro.getMap("ha");
  b.set("ab", 123);
  loro.commit();

  const bText = b.insertContainer("hh", "Text");
  loro.commit();

  it("map get", () => {
    assertEquals(b.get("ab"), 123);
  });

  it("getValueDeep", () => {
    bText.insert(0, "hello world Text");
    assertEquals(b.getDeepValue(), { ab: 123, hh: "hello world Text" });
  });

  it("get container by id", () => {
    const id = b.id;
    const b2 = loro.getContainerById(id) as LoroMap;
    assertEquals(b2.value, b.value);
    assertEquals(b2.id, id);
    b2.set("0", 12);

    assertEquals(b2.value, b.value);
  });
});

describe("type", () => {
  it("test map type", () => {
    const loro = new Loro<{ map: LoroMap<{ name: "he" }> }>();
    const map = loro.getTypedMap("map");
    const v = map.getTyped(loro, "name");
    expectTypeOf(v).toEqualTypeOf<"he">();
  });

  it("test recursive map type", () => {
    const loro = new Loro<{ map: LoroMap<{ map: LoroMap<{ name: "he" }> }> }>();
    const map = loro.getTypedMap("map");
    map.insertContainer("map", "Map");

    const subMap = map.getTyped(loro, "map");
    const name = subMap.getTyped(loro, "name");
    expectTypeOf(name).toEqualTypeOf<"he">();
  });

  it("works for list type", () => {
    const loro = new Loro<{ list: LoroList<[string, number]> }>();
    const list = loro.getTypedList("list");
    console.dir((list as any).__proto__);
    list.insertTyped(0, "123");
    list.insertTyped(1, 123);
    const v0 = list.getTyped(loro, 0);
    expectTypeOf(v0).toEqualTypeOf<string>();
    const v1 = list.getTyped(loro, 1);
    expectTypeOf(v1).toEqualTypeOf<number>();
  });

  it("test binary type", () => {
    // const loro = new Loro<{ list: LoroList<[string, number]> }>();
    // const list = loro.getTypedList("list");
    // console.dir((list as any).__proto__);
    // list.insertTyped(0, new Uint8Array(10));
    // const v0 = list.getTyped(loro, 0);
    // expectTypeOf(v0).toEqualTypeOf<Uint8Array>();
  });
});

describe("tree", () => {
  const loro = new Loro();
  const tree = loro.getTree("root");

  it("create move", () => {
    const id = tree.create();
    const childID = tree.create(id);
    console.log(typeof id);
    assertEquals(tree.parent(childID), id);
  });

  it("meta", () => {
    const id = tree.create();
    const meta = tree.getMeta(id);
    meta.set("a", 123);
    assertEquals(meta.get("a"), 123);
  });
});

function one_ms(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 1));
}
