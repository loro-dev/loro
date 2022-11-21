import init, { Loro, setPanicHook } from "../pkg/loro_wasm.js";
import { resolve } from "https://deno.land/std@0.105.0/path/mod.ts";
import __ from "https://deno.land/x/dirname@1.1.2/mod.ts";
import { assertEquals, assertThrows } from "https://deno.land/std@0.165.0/testing/asserts.ts";
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
  console.log(a.value);
  const b = loro.getMap("ha");
  b.set(loro, "ab", 123);
  console.log(b.value);
  console.log(a.value);
  const bText = b.getText(loro, "hh");
  await t.step("getValueDeep", () => {
    bText.insert(loro, 0, "hello world Text");
    assertEquals(b.getValueDeep(loro), { ab: 123, hh: "hello world Text" });
  });

  await t.step("wrong context throw error", () => {
    assertThrows(()=>{
      const loro2 = new Loro();
      bText.insert(loro2, 0, "hello world Text");
    });
  });
  
  await t.step("get value error", () => {
    assertThrows(()=>{
      const _ = bText.value;
    });
  });
});
