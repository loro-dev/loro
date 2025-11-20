---
"loro-crdt": minor
"loro-crdt-map": minor
---

feat: add sliceDelta method to slice a span of richtext #862

Use `text.sliceDelta(start, end)` when you need a Quill-style delta for only part of a rich text field (for example, to copy a styled snippet). The method takes UTF-16 indices; use `sliceDeltaUtf8` if you want to slice by UTF-8 byte offsets instead.

```ts
import { LoroDoc } from "loro-crdt";

const doc = new LoroDoc();
doc.configTextStyle({
  bold: { expand: "after" },
  comment: { expand: "none" },
});
const text = doc.getText("text");

text.insert(0, "Hello World!");
text.mark({ start: 0, end: 5 }, "bold", true);
text.mark({ start: 6, end: 11 }, "comment", "greeting");

const snippet = text.sliceDelta(1, 8);
console.log(snippet);
// [
//   { insert: "ello", attributes: { bold: true } },
//   { insert: " " },
//   { insert: "Wo", attributes: { comment: "greeting" } },
// ]
```
