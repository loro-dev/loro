import init, {
  Loro,
  PrelimList,
  PrelimMap,
  PrelimText,
  setPanicHook,
} from "../pkg/loro_wasm.js";
import { resolve } from "https://deno.land/std@0.105.0/path/mod.ts";
import __ from "https://deno.land/x/dirname@1.1.2/mod.ts";
import {
  assertEquals,
  assertThrows,
} from "https://deno.land/std@0.165.0/testing/asserts.ts";
const { __dirname } = __(import.meta);

const wasm = await Deno.readFile(
  resolve(__dirname, "../pkg/loro_wasm_bg.wasm"),
);

Deno.test({
  name: "loro_wasm",
}, async (t) => {
  await init(wasm);
  setPanicHook();
  const loro = new Loro();
  const a = loro.getText("ha");
  a.insert(loro, 0, "hello world");
  a.delete(loro, 6, 5);
  a.insert(loro, 6, "everyone");
  const b = loro.getMap("ha");
  b.set(loro, "ab", 123);

  const bText = b.setContainer(loro, "hh", "text");

  await t.step("map get", () => {
    assertEquals(b.get("ab"), 123);
  });

  await t.step("getValueDeep", () => {
    bText.insert(loro, 0, "hello world Text");
    assertEquals(b.getValueDeep(loro), { ab: 123, hh: "hello world Text" });
  });

  await t.step("wrong context throw error", () => {
    assertThrows(() => {
      const loro2 = new Loro();
      bText.insert(loro2, 0, "hello world Text");
    });
  });
});

Deno.test({ name: "test prelim" }, async (t) => {
  await init(wasm);
  setPanicHook();
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
