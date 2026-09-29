/* eslint-disable no-console */
// Writes the Rust (loro-crdt) side of the rich-text interop fixtures used by
// tests/richtext-anchors.test.ts. Build the nodejs WASM package first
// (`pnpm -C crates/loro-wasm build-dev`) or point LORO_WASM_NODEJS at one.
const { writeFileSync } = require("node:fs");
const path = require("node:path");

const wasmPath =
  process.env.LORO_WASM_NODEJS ??
  path.resolve(__dirname, "../../crates/loro-wasm/nodejs/index.js");
const { LoroDoc } = require(wasmPath);
const fixture = (name) => path.resolve(__dirname, "../tests/fixtures/rust", name);

function newDoc() {
  const doc = new LoroDoc();
  doc.setPeerId(1);
  doc.configTextStyle({ bold: { expand: "after" }, link: { expand: "none" } });
  return doc;
}

const scenarios = {
  // loro-dev/loro#1133: positions written after a mark count its anchors.
  "richtext-mark-positions"(doc) {
    const text = doc.getText("text");
    text.insert(0, "abcd");
    doc.commit();
    text.mark({ start: 0, end: 2 }, "bold", true);
    doc.commit();
    text.insert(3, "X");
    doc.commit();
    text.delete(1, 1);
    doc.commit();
  },
  "richtext-unmark-emoji"(doc) {
    const text = doc.getText("text");
    text.insert(0, "a😀bc");
    doc.commit();
    text.mark({ start: 0, end: 3 }, "bold", true);
    doc.commit();
    text.unmark({ start: 0, end: 1 }, "bold");
    doc.commit();
    text.delete(3, 1);
    doc.commit();
  },
  "richtext-checkout-mark"(doc, versions) {
    const text = doc.getText("text");
    text.insert(0, "abc");
    doc.commit();
    versions.push(doc.frontiers());
    text.mark({ start: 0, end: 3 }, "bold", true);
    text.insert(3, "d");
    doc.commit();
  },
};

for (const [name, edit] of Object.entries(scenarios)) {
  const doc = newDoc();
  const versions = [];
  edit(doc, versions);
  const checkouts = versions.map((frontiers) => {
    doc.checkout(frontiers);
    const delta = doc.getText("text").toDelta();
    doc.checkoutToLatest();
    return { frontiers, delta };
  });
  writeFileSync(
    fixture(`${name}.json`),
    `${JSON.stringify(
      {
        delta: doc.getText("text").toDelta(),
        checkouts,
        updates: doc.exportJsonUpdates(),
      },
      null,
      2,
    )}\n`,
  );
  writeFileSync(fixture(`${name}.blob`), doc.export({ mode: "update" }));
  writeFileSync(fixture(`${name}.snapshot.blob`), doc.export({ mode: "snapshot" }));
  console.log(name, JSON.stringify(doc.getText("text").toDelta()));
}
