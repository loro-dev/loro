import { EphemeralStoreWasm, LoroDoc } from "loro-crdt";

// Check every published entry's declarations without loading its runtime.
import "loro-crdt/bundler";
import "loro-crdt/nodejs";
import "loro-crdt/web";
import "loro-crdt/browser";
import "loro-crdt/base64";

// Issue #1179's consumer, plus the three affected public APIs.
const doc = new LoroDoc();
doc.getText("text").insert(0, "Hello");
console.log(doc.toJSON());

doc.toJsonWithReplacer((key, value) => {
  if (typeof key === "number") {
    key.toFixed();
  } else {
    key.toUpperCase();
  }
  return value;
});
doc.getText("text").iter((span: string): boolean => span.length > 0);
const store = new EphemeralStoreWasm(1000);
const removed: void = store.removeOutdated();
