import { assert, describe, expect, it } from "vitest";
import { LoroDoc, LoroList, LoroMap, LoroText, VersionVector } from "../src";
import { expectTypeOf } from "vitest";

function assertEquals(a: any, b: any) {
  expect(a).toStrictEqual(b);
}

describe("transaction", () => {
  it("transaction", async () => {
    const loro = new LoroDoc();
    const text = loro.getText("text");
    let count = 0;
    const sub = loro.subscribe(() => {
      count += 1;
      sub();
    });
    expect(count).toBe(0);
    text.insert(0, "hello world");
    expect(count).toBe(0);
    text.insert(0, "hello world");
    assertEquals(count, 0);
    loro.commit();
    await one_ms();
    assertEquals(count, 1);
  });

  it("transaction origin", async () => {
    const loro = new LoroDoc();
    const text = loro.getText("text");
    let count = 0;
    const sub = loro.subscribe((event: { origin: string }) => {
      count += 1;
      sub();
      assertEquals(event.origin, "origin");
    });

    assertEquals(count, 0);
    text.insert(0, "hello world");
    assertEquals(count, 0);
    text.insert(0, "hello world");
    assertEquals(count, 0);
    loro.commit({ origin: "origin" });
    await one_ms();
    assertEquals(count, 1);
  });
});

describe("subscribe", () => {
  it("subscribe_lock", async () => {
    const loro = new LoroDoc();
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
    await one_ms();

    assertEquals(count, 2);
    text.insert(0, "hello world");
    loro.commit();
    await one_ms();
    assertEquals(count, 3);
    sub();
    text.insert(0, "hello world");
    loro.commit();
    await one_ms();
    assertEquals(count, 3);
  });

  it("subscribe_lock2", async () => {
    const loro = new LoroDoc();
    const text = loro.getText("text");
    let count = 0;
    const sub = loro.subscribe(() => {
      count += 1;
      sub()
    });
    assertEquals(count, 0);
    text.insert(0, "hello world");
    loro.commit();
    await one_ms();

    assertEquals(count, 1);
    text.insert(0, "hello world");
    loro.commit();
    await one_ms();

    assertEquals(count, 1);
  });

  it("subscribe", async () => {
    const loro = new LoroDoc();
    const text = loro.getText("text");
    let count = 0;
    const sub = loro.subscribe(() => {
      count += 1;
    });
    text.insert(0, "hello world");
    loro.commit();
    await one_ms();
    assertEquals(count, 1);
    text.insert(0, "hello world");
    loro.commit();
    await one_ms();
    assertEquals(count, 2);
    sub();
    text.insert(0, "hello world");
    loro.commit();
    await one_ms();
    assertEquals(count, 2);
  });
});

describe("sync", () => {
  it("two insert at beginning", async () => {
    const a = new LoroDoc();
    const b = new LoroDoc();
    let a_version: undefined | VersionVector = undefined;
    let b_version: undefined | VersionVector = undefined;
    a.subscribe((e) => {
      if (e.by == "local") {
        const exported = a.exportFrom(a_version);
        b.import(exported);
        a_version = a.version();
      }
    });
    b.subscribe((e) => {
      if (e.by == "local") {
        const exported = b.exportFrom(b_version);
        a.import(exported);
        b_version = b.version();
      }
    });
    const aText = a.getText("text");
    const bText = b.getText("text");
    aText.insert(0, "abc");
    a.commit();
    await one_ms();

    assertEquals(aText.toString(), bText.toString());
  });

  it("sync", () => {
    const loro = new LoroDoc();
    const text = loro.getText("text");
    text.insert(0, "hello world");

    const loro_bk = new LoroDoc();
    loro_bk.import(loro.exportFrom(undefined));
    assertEquals(loro_bk.toJSON(), loro.toJSON());
    const text_bk = loro_bk.getText("text");
    assertEquals(text_bk.toString(), "hello world");
    text_bk.insert(0, "a ");

    loro.import(loro_bk.exportFrom(undefined));
    assertEquals(text.toString(), "a hello world");
    const map = loro.getMap("map");
    map.set("key", "value");
  });
});

describe("wasm", () => {
  const loro = new LoroDoc();
  const a = loro.getText("ha");
  a.insert(0, "hello world");
  a.delete(6, 5);
  a.insert(6, "everyone");
  loro.commit();

  const b = loro.getMap("ha");
  b.set("ab", 123);
  loro.commit();

  const bText = b.setContainer("hh", new LoroText());
  loro.commit();

  it("map get", () => {
    assertEquals(b.get("ab"), 123);
  });

  it("getValueDeep", () => {
    bText.insert(0, "hello world Text");
    assertEquals(b.toJSON(), { ab: 123, hh: "hello world Text" });
  });

  it("get container by id", () => {
    const id = b.id;
    const b2 = loro.getContainerById(id) as LoroMap;
    assertEquals(b2.toJSON(), b.toJSON());
    assertEquals(b2.id, id);
    b2.set("0", 12);

    assertEquals(b2.toJSON(), b.toJSON());
  });
});

describe("type", () => {
  it("test map type", () => {
    const loro = new LoroDoc<{ map: LoroMap<{ name: "he" }> }>();
    const map = loro.getMap("map");
    const v = map.get("name");
    expectTypeOf(v).toEqualTypeOf<"he">();
  });

  it("test recursive map type", () => {
    const loro = new LoroDoc<{ map: LoroMap<{ map: LoroMap<{ name: "he" }> }> }>();
    const map = loro.getMap("map");
    map.setContainer("map", new LoroMap());

    const subMap = map.get("map");
    const name = subMap.get("name");
    expectTypeOf(name).toEqualTypeOf<"he">();
  });

  it("works for list type", () => {
    const loro = new LoroDoc<{ list: LoroList<string> }>();
    const list = loro.getList("list");
    list.insert(0, "123");
    const v0 = list.get(0);
    expectTypeOf(v0).toEqualTypeOf<string>();
  });

  it("test binary type", () => {
    const loro = new LoroDoc<{ list: LoroList<Uint8Array> }>();
    const list = loro.getList("list");
    list.insert(0, new Uint8Array(10));
    const v0 = list.get(0);
    expectTypeOf(v0).toEqualTypeOf<Uint8Array>();
  });
});



describe("list stable position", () => {
  it("basic tests", () => {
    const loro = new LoroDoc();
    const list = loro.getList("list");
    list.insert(0, "a");
    const pos0 = list.getCursor(0);
    list.insert(1, "b");
    {
      const ans = loro.getCursorPos(pos0!);
      expect(ans.offset).toEqual(0);
      expect(ans.side).toEqual(0);
      expect(ans.update).toBeUndefined();
    }
    list.insert(0, "c");
    {
      const ans = loro.getCursorPos(pos0!);
      expect(ans.offset).toEqual(1);
      expect(ans.side).toEqual(0);
      expect(ans.update).toBeUndefined();
    }
    list.delete(1, 1);
    {
      const ans = loro.getCursorPos(pos0!);
      expect(ans.offset).toEqual(1);
      expect(ans.side).toEqual(-1);
      expect(ans.update).toBeDefined();
    }
  });
});

describe("to json", () => {
  it("to shallow json", async () => {
    const loro = new LoroDoc();
    loro.getText("text");
    loro.getMap("map");
    loro.getList("list");
    loro.getTree("tree");
    loro.getMovableList("movable_list");
    const value = loro.getShallowValue();
    assert(Object.keys(value).includes("text"));
    assert(Object.keys(value).includes("map"));
    assert(Object.keys(value).includes("list"));
    assert(Object.keys(value).includes("tree"));
    assert(Object.keys(value).includes("movable_list"));
  });
});

function one_ms(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 1));
}
