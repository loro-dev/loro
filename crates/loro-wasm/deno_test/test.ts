import init, { Loro } from "../pkg/loro_wasm.js";
const wasm = await Deno.readFile("../pkg/loro_wasm_bg.wasm");

await init(wasm);
const loro = new Loro();
const a = loro.get_text_container("ha");
a.insert(0, "hello world");
a.delete(6, 5);
a.insert(6, "everyone");
console.log(a.get_value());
