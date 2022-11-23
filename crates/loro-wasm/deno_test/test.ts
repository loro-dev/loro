import init, { Loro, setPanicHook } from "../pkg/loro_wasm.js";
import { resolve } from "https://deno.land/std@0.105.0/path/mod.ts";
import __ from "https://deno.land/x/dirname@1.1.2/mod.ts";
const { __dirname } = __(import.meta);

const wasm = await Deno.readFile(
  resolve(__dirname, "../pkg/loro_wasm_bg.wasm")
);

await init(wasm);
setPanicHook();
const loro = new Loro();
const a = loro.getText("ha");
a.insert(loro, 0, "hello world");
a.delete(loro,6, 5);
a.insert(loro,6, "everyone");
console.log(a.value);
const b = loro.getMap("ha");
b.set(loro,"ab", 123);
console.log(b.value);
console.log(a.value);
let bText = b.getText(loro, "hh");
bText.insert(loro, 0, "hello world Text");
console.log(b.getValueDeep(loro));
// console.log(b.value);