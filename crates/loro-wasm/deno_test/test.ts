import init, {
  enableDebug,
  Loro,
  LoroMap,
  PrelimList,
  PrelimMap,
  PrelimText,
  setPanicHook,
Transaction,
} from "../web/loro_wasm.js";
import {
  assertEquals,
  assertThrows,
} from "https://deno.land/std@0.165.0/testing/asserts.ts";

await init();
setPanicHook();
enableDebug();

Deno.test({
  name: "loro_wasm",
}, async (t) => {
  const loro = new Loro();
  const a = loro.getText("ha");
  a.insert(loro, 0, "hello world");
  a.delete(loro, 6, 5);
  a.insert(loro, 6, "everyone");
  const b = loro.getMap("ha");
  b.set(loro, "ab", 123);

  const bText = b.insertContainer(loro, "hh", "text");

  await t.step("map get", () => {
    assertEquals(b.get("ab"), 123);
  });

  await t.step("getValueDeep", () => {
    bText.insert(loro, 0, "hello world Text");
    assertEquals(b.getValueDeep(loro), { ab: 123, hh: "hello world Text" });
  });

  await t.step("should throw error when using the wrong context", () => {
    assertThrows(() => {
      const loro2 = new Loro();
      bText.insert(loro2, 0, "hello world Text");
    });
  });

  await t.step("get container by id", () => {
    const id = b.id;
    const b2 = loro.getContainerById(id) as LoroMap;
    assertEquals(b2.value, b.value);
    assertEquals(b2.id, id);
    b2.set(loro, "0", 12);
    assertEquals(b2.value, b.value);
  });
});

Deno.test({ name: "sync" }, async (t) => {
  await t.step("two insert at beginning", () => {
    const a = new Loro();
    const b = new Loro();
    let a_version: undefined | Uint8Array = undefined;
    let b_version: undefined | Uint8Array = undefined;
    a.subscribe((local: boolean) => {
      if (local) {
        const exported = a.exportUpdates(a_version);
        b.importUpdates(exported);
        a_version = a.version();
      }
    });
    b.subscribe((local: boolean) => {
      if (local) {
        const exported = b.exportUpdates(b_version);
        a.importUpdates(exported);
        b_version = b.version();
      }
    });
    const aText = a.getText("text");
    const bText = b.getText("text");
    aText.insert(a, 0, "abc");
    assertEquals(aText.toString(), bText.toString());
  });

  await t.step("sync", () => {
    const loro = new Loro();
    const text = loro.getText("text");
    text.insert(loro, 0, "hello world");
    const loro_bk = new Loro();
    loro_bk.importUpdates(loro.exportUpdates(undefined));
    assertEquals(loro_bk.toJson(), loro.toJson());
    const text_bk = loro_bk.getText("text");
    assertEquals(text_bk.toString(), "hello world");
    text_bk.insert(loro_bk, 0, "a ");
    loro.importUpdates(loro_bk.exportUpdates(undefined));
    assertEquals(text.toString(), "a hello world");
    const map = loro.getMap("map");
    map.set(loro, "key", "value");
  });
});

Deno.test("subscribe", () => {
  const loro = new Loro();
  const text = loro.getText("text");
  let count = 0;
  const sub = loro.subscribe(() => {
    count += 1;
  });
  text.insert(loro, 0, "hello world");
  assertEquals(count, 1);
  text.insert(loro, 0, "hello world");
  assertEquals(count, 2);
  loro.unsubscribe(sub);
  text.insert(loro, 0, "hello world");
  assertEquals(count, 2);
});

Deno.test({ name: "test prelim" }, async (t) => {
  const loro = new Loro();
  const map = loro.getMap("map");
  const list = loro.getList("list");
  const prelim_text = new PrelimText(undefined);
  const prelim_map = new PrelimMap({ a: 1, b: 2 });
  const prelim_list = new PrelimList([1, "2", { a: 4 }]);

  await t.step("prelim text", () => {
    prelim_text.insert(0, "hello world");
    assertEquals(prelim_text.value, "hello world");
    prelim_text.delete(6, 5);
    prelim_text.insert(6, "everyone");
    assertEquals(prelim_text.value, "hello everyone");
  });

  await t.step("prelim map", () => {
    prelim_map.set("ab", 123);
    assertEquals(prelim_map.value, { a: 1, b: 2, ab: 123 });
    prelim_map.delete("b");
    assertEquals(prelim_map.value, { a: 1, ab: 123 });
  });

  await t.step("prelim list", () => {
    prelim_list.insert(0, 0);
    assertEquals(prelim_list.value, [0, 1, "2", { a: 4 }]);
    prelim_list.delete(1, 2);
    assertEquals(prelim_list.value, [0, { a: 4 }]);
  });

  await t.step("prelim map integrate", () => {
    map.set(loro, "text", prelim_text);
    map.set(loro, "map", prelim_map);
    map.set(loro, "list", prelim_list);
    assertEquals(map.getValueDeep(loro), {
      text: "hello everyone",
      map: { a: 1, ab: 123 },
      list: [0, { a: 4 }],
    });
  });

  await t.step("prelim list integrate", () => {
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

Deno.test("subscribe_lock", () => {
  const loro = new Loro();
  const text = loro.getText("text");
  const list = loro.getList("list");
  let count = 0;
  let i = 1;
  const sub = loro.subscribe(() => {
    if (i >0){
      list.insert(loro, 0, i);
      i--;
    }
    count += 1;
  });
  text.insert(loro, 0, "hello world");
  assertEquals(count, 2);
  text.insert(loro, 0, "hello world");
  assertEquals(count, 3);
  loro.unsubscribe(sub);
  text.insert(loro, 0, "hello world");
  assertEquals(count, 3);
});

Deno.test("subscribe_lock2", () => {
  const loro = new Loro();
  const text = loro.getText("text");
  let count = 0;
  const sub = loro.subscribe(() => {
    count += 1;
    loro.unsubscribe(sub);
  });
  assertEquals(count, 0);
  text.insert(loro, 0, "hello world");
  assertEquals(count, 1);
  text.insert(loro, 0, "hello world");
  assertEquals(count, 1);
});

Deno.test("transaction", () => {
  const loro = new Loro();
  const text = loro.getText("text");
  let count = 0;
  const sub = loro.subscribe(() => {
    count += 1;
    loro.unsubscribe(sub);
  });
  loro.transaction((txn: Transaction)=>{
    assertEquals(count, 0);
    text.insert(txn, 0, "hello world");
    assertEquals(count, 0);
    text.insert(txn, 0, "hello world");
    assertEquals(count, 0);
  });
  assertEquals(count, 1);
});
