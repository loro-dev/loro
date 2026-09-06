// Run with --no-experimental-require-module to catch ESM leaking into CJS.
const assert = require("node:assert/strict");
const { LoroDoc, LoroText } = require("../nodejs");
const doc = new LoroDoc();
const map = doc.getMap("root");
const text = map.setContainer("text", new LoroText());
text.insert(0, "hello");
text.mark({ start: 0, end: 5 }, "bold", true);
map.set("bytes", new Uint8Array([0, 255]));
assert.deepEqual(doc.toJSON().root.text, "hello");
const result = doc.toContainerTree({ text: "delta" }).root;
assert.equal(result.cid, map.id);
assert.deepEqual(result.value.text, {
  type: "Text",
  cid: text.id,
  value: text.toDelta(),
});
assert.deepEqual(result.value.bytes.value, new Uint8Array([0, 255]));
doc.free();
console.log(
  "CommonJS loading and container tree smoke passed without require(esm)",
);
