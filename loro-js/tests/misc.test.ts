import { assertType, describe, expect, it } from "vitest";
import {
  Loro,
  LoroList,
  LoroMap,
  PrelimList,
  PrelimMap,
  PrelimText,
  Transaction,
} from "../src";
import { expectTypeOf } from "vitest";
import { assert } from "https://lra6z45nakk5lnu3yjchp7tftsdnwwikwr65ocha5eojfnlgu4sa.arweave.net/XEHs860CldW2m8JEd_5lnIbbWQq0fdcI4OkckrVmpyQ/_util/assert.ts";

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
        loro.transact(txn => {
          list.insert(txn, 0, i);
          i--;
        })
      }

      count += 1;
    });
    loro.transact((txn) => {
      text.insert(txn, 0, "hello world");
    })

    assertEquals(count, 2);
    loro.transact((txn) => {
      text.insert(txn, 0, "hello world");
    });
    assertEquals(count, 3);
    loro.unsubscribe(sub);
    loro.transact(txn => {
      text.insert(txn, 0, "hello world");
    })
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
    loro.transact(txn => {
      text.insert(txn, 0, "hello world");
    })

    assertEquals(count, 1);
    loro.transact(txn => {
      text.insert(txn, 0, "hello world");
    })

    assertEquals(count, 1);
  });

  it("subscribe", async () => {
    const loro = new Loro();
    const text = loro.getText("text");
    let count = 0;
    const sub = loro.subscribe(() => {
      count += 1;
    });
    loro.transact(loro => {
      text.insert(loro, 0, "hello world");
    })
    assertEquals(count, 1);
    loro.transact(loro => {
      text.insert(loro, 0, "hello world");
    })
    assertEquals(count, 2);
    loro.unsubscribe(sub);
    loro.transact(loro => {
      text.insert(loro, 0, "hello world");
    })
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
    a.transact(txn => {
      aText.insert(txn, 0, "abc");
    });

    assertEquals(aText.toString(), bText.toString());
  });

  it("sync", () => {
    const loro = new Loro();
    const text = loro.getText("text");
    loro.transact(txn => {
      text.insert(txn, 0, "hello world");
    });

    const loro_bk = new Loro();
    loro_bk.import(loro.exportFrom(undefined));
    assertEquals(loro_bk.toJson(), loro.toJson());
    const text_bk = loro_bk.getText("text");
    assertEquals(text_bk.toString(), "hello world");
    loro_bk.transact(txn => {
      text_bk.insert(txn, 0, "a ");
    });

    loro.import(loro_bk.exportFrom(undefined));
    assertEquals(text.toString(), "a hello world");
    const map = loro.getMap("map");
    loro.transact(txn => {
      map.set(txn, "key", "value");
    });

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
      loro.transact(txn => {
        map.set(txn, "text", prelim_text);
        map.set(txn, "map", prelim_map);
        map.set(txn, "list", prelim_list);
      });

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
      loro.transact(txn => {
        list.insert(txn, 0, prelim_text);
        list.insert(txn, 1, prelim_map);
        list.insert(txn, 2, prelim_list);
      });

      assertEquals(list.getDeepValue(), ["ttt", { a: 1, b: 2 }, [1, "2", {
        a: 4,
      }]]);
    });
  });
});

describe("wasm", () => {
  const loro = new Loro();
  const a = loro.getText("ha");
  loro.transact(txn => {
    a.insert(txn, 0, "hello world");

    a.delete(txn, 6, 5);
    a.insert(txn, 6, "everyone");
  });
  const b = loro.getMap("ha");
  loro.transact(txn => {
    b.set(txn, "ab", 123);
  });

  const bText = loro.transact(txn => {
    return b.insertContainer(txn, "hh", "Text")
  });

  it("map get", () => {
    assertEquals(b.get("ab"), 123);
  });

  it("getValueDeep", () => {
    loro.transact(txn => {
      bText.insert(txn, 0, "hello world Text");
    });

    assertEquals(b.getDeepValue(), { ab: 123, hh: "hello world Text" });
  });

  it("should throw error when using the wrong context", () => {
    expect(() => {
      const loro2 = new Loro();
      loro2.transact(txn => {
        bText.insert(txn, 0, "hello world Text");
      });

    }).toThrow();
  });

  it("get container by id", () => {
    const id = b.id;
    const b2 = loro.getContainerById(id) as LoroMap;
    assertEquals(b2.value, b.value);
    assertEquals(b2.id, id);
    loro.transact(txn => {
      b2.set(txn, "0", 12);
    });

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
    loro.transact(txn => {
      map.insertContainer(txn, "map", "Map");
    });

    const subMap = map.getTyped(loro, "map");
    const name = subMap.getTyped(loro, "name");
    expectTypeOf(name).toEqualTypeOf<"he">();
  });

  it("works for list type", () => {
    const loro = new Loro<{ list: LoroList<[string, number]> }>();
    const list = loro.getTypedList("list");
    console.dir((list as any).__proto__);
    loro.transact(txn => {
      list.insertTyped(txn, 0, "123");
    });

    loro.transact(txn => {
      list.insertTyped(txn, 1, 123);
    });

    const v0 = list.getTyped(loro, 0);
    expectTypeOf(v0).toEqualTypeOf<string>();
    const v1 = list.getTyped(loro, 1);
    expectTypeOf(v1).toEqualTypeOf<number>();
  });

  it("test binary type", () => {
    const loro = new Loro<{ list: LoroList<[string, number]> }>();
    const list = loro.getTypedList("list");
    console.dir((list as any).__proto__);
    loro.transact(txn => {
      list.insertTyped(txn, 0, new Uint8Array(10));
    });
    const v0 = list.getTyped(loro, 0);
    expectTypeOf(v0).toEqualTypeOf<Uint8Array>();
  });
});

describe("tree", () => {
  const loro = new Loro();
  const tree = loro.getTree("root");
  
  it("create move", ()=>{
    const id = loro.transact((txn)=>{
      return tree.create(txn);
    })
    const childID = loro.transact((txn)=>{
      return tree.create(txn, id);
    })
    console.log(typeof id);
    
    assertEquals(tree.parent(childID), id);
  })
})

function one_ms(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 1));
}
