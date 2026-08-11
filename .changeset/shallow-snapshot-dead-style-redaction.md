---
"loro-crdt": patch
---

Shallow snapshot export no longer leaks the values of rich-text marks whose
whole range was deleted before the shallow root.

Deleting styled text removes the characters but keeps the style anchors, and the
shallow-root state encoding shipped each anchor's value verbatim — so a mark
value (e.g. an inline comment) that every read API already reported as gone
still appeared in the exported bytes, defeating the documented content-redaction
use of shallow snapshots. Dead style pairs now have their values nulled during
export (anchors stay, so positions and replay are unaffected). Styles that only
become dead after the shallow root keep their values so historical checkouts
render correctly, and styles configured with `expand: "both"` are kept because
an empty both-expand pair still applies to future inserts. Re-exporting a
shallow snapshot produced by an older version also cleans it.
