import init, { Loro } from "../pkg/loro_wasm.js";
import { resolve } from "https://deno.land/std@0.105.0/path/mod.ts";
import __ from "https://deno.land/x/dirname@1.1.2/mod.ts";
const { __dirname } = __(import.meta);

const wasm = await Deno.readFile(
  resolve(__dirname, "../pkg/loro_wasm_bg.wasm")
);

await init(wasm);
const loro = new Loro();
const a = loro.get_text_container("ha");
a.insert(0, "hello world");
a.delete(6, 5);
a.insert(6, "everyone");
console.log(a.get_value());
const b = loro.get_map_container("ha");
b.set("ab", 123);
console.log(b.get_value());
console.log(a.get_value());
