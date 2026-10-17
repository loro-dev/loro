# Deep plain values and stack safety

Verified against code 2026-10-17.

Issue [#1198](https://github.com/loro-dev/loro/issues/1198) constructs a plain
`LoroValue::Map` chain with 20,000 levels. On main `3827945f`, its release-mode
reproduction exports 848 bytes, imports successfully, then aborts on a 1 MiB
reader stack. LZ4 compresses the repeated nesting, so limiting file bytes does
not bound value depth.

`state/value_decode.rs::decode_collection` reads the existing postcard value
layout with heap frames. A shallow `Header` consumes a scalar or just a
collection's length; children are read by the outer loop. Map, List and
MovableList fast snapshot values use this reader, including the ordered lazy
map cache in `MapState::decode_value_as_btree_map`. Its layout test compares
against the existing serde serializer, including container IDs and every scalar
kind. Import stays lazy and the binary format is unchanged. There is no new
failure after advancing the oplog: deep snapshot values remain valid and readable.

The general `Deserialize for LoroValue` in `loro-common/src/value.rs` uses a
`DeserializeSeed` budget of 128 collection levels, shared down both map and list
children. This protects non-snapshot serde callers without relying on the
format's own limit. Binary snapshot reads use the iterative reader above and
therefore continue supporting deeper existing values, including rich-text mark values. Postcard represents custom
serde errors as `SerdeDeCustom`; formats such as JSON retain the depth message.

`LoroMapValue` and `LoroListValue` detach owned children and drop them using a
work list. Shared allocations use `Arc::into_inner`, not a failed
`Arc::try_unwrap(...).ok()`, so simultaneous final owners cannot recursively
free an arbitrarily deep chain. Empty replacement Arcs are shared to avoid an
allocation for each shared-value drop. `unwrap()` still returns the existing
owned map/vector API. Error cleanup of partially decoded frames uses the same
safe destruction path.

The `LoroListValue::into_arc` API added by #1197 keeps the same allocation by
using `mem::take(&mut self.0)`. Both list/map `unwrap` methods likewise take the
Arc before `try_unwrap`, cloning the contents only when shared. These wrappers
implement Drop, so their fields must never be moved directly out of `self`.

WASM's `loro_common::try_loro_value_to_js_value` uses heap tasks to build JS
objects/arrays. Both `From<LoroValue> for JsValue` and the fallible binding
converter use it; fixing postcard alone would leave first reads vulnerable in
this conversion.

Regression fixtures `crates/loro/tests/deep_value_{map,lists}.bin` use peer 1198
and 20,000 levels ending in `I64(0)`. The map fixture is the issue's `root.v`
chain. The lists fixture stores a chain in both `list[0]` and `movable[0]`, built
bottom-up with `List([value])` at even iterations and `Map({a: value})` at odd
iterations. They were exported on a 1 GiB producer stack using the existing
snapshot serializer. Tests read them on a 1 MiB native stack and in the packaged
WASM build, walk the leaves iteratively, and actually free the values/documents.
They also check state/oplog frontiers and lazy snapshot re-export. The fixture
avoids requiring a large producer stack in the regression test itself.

Rich-text `EncodedText::decode` in `state/richtext_state.rs` reads the unchanged
three-field outer sequence and three-field `EncodedMark` sequences directly.
Spans retain their existing columnar strategies; only each mark value goes
through `value_decode::decode_value`. Both first state access and
`redact_dead_style_values` use this path. `EncodedText::encode` uses the matching
iterative value writer, so rewriting a dead style does not recursively serialize
another, still-live deep style. A unit test compares the complete output bytes
with the original serde-columnar serializer.

The 20,000-level mark regression also exercises binary op export and rich-text
style checks. `encoding/value.rs::ValueWriter::write_value_content` now uses
heap tasks, retaining the original key-register traversal order. `LoroValue::hash`
also uses heap tasks because style sets hash the full value when the decoded
text state is materialized. It keeps list length prefixes, sorted map keys,
normalized NaNs/zero and the existing scalar hashes. Neither change relaxes the
external serde depth budget.

`crates/loro/tests/deep_mark_snapshot.rs` constructs and exports marks at depths
129, 200 and 20,000 entirely on a 1 MiB stack, then reads snapshot, shallow and
state-only round trips. Each source includes both a live deep mark and a deep
mark whose text was deleted, forcing shallow redaction to decode both and
rewrite the retained live value. It also covers remote binary updates, verifies
the leaf and state/oplog frontiers, and edits/exports an unrelated map after
reads. WASM `deep-mark.test.ts` covers 129-level marks and verifies that a rejected
object JSON import still leaves its target usable.
