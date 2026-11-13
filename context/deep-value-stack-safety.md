# Deep plain values and stack safety

Verified against code 2026-11-06.

Issue [#1198](https://github.com/loro-dev/loro/issues/1198) constructs a plain
`LoroValue::Map` chain with 20,000 levels. On main `3827945f`, its release-mode
reproduction exports 848 bytes, imports successfully, then aborts on a 1 MiB
reader stack. LZ4 compresses the repeated nesting, so limiting file bytes does
not bound value depth.

`state/value_decode.rs::decode_collection` reads the existing postcard value
layout with heap frames. A shallow `Header` consumes a scalar or just a
collection's length; children are read by the outer loop. Map, List and
MovableList fast snapshot values use this reader. The ordered lazy map cache
in `MapState::decode_value_as_btree_map` uses `decode_btree_map`: it reads the
bare length and keys directly into a `BTreeMap`, while each value uses the same
iterative `decode_value` reader. Scalars return directly after their shallow
header; only collection values enter the heap-frame loop. Each root reader shares
one postcard slice deserializer across its keys, headers and children, then
finalizes once to return remaining metadata. Errors retain the same decode-error
mapping and never return a partially consumed remainder. This avoids constructing an intermediate
`FxHashMap` and then rebuilding an ordered map on the first key read. Neither
reader reserves storage from untrusted collection lengths. Duplicate keys retain
the last value and trailing state metadata is returned unchanged. Layout,
truncation/invalid-length and 20,000-level small-stack/error-cleanup tests cover
this ordered path. Its layout test compares
against the existing serde serializer, including container IDs and every scalar
kind. Import stays lazy and the binary format is unchanged. There is no new
failure after advancing the oplog: deep snapshot values remain valid and readable.

Equality follows the same rule. `values_eq` in `loro-common/src/value.rs` and the
`JsonLossy` comparison in `oplog/known_history.rs` (`ImportedValues::eq`) use a work
list. Every comparison of value trees on an import path must, because imports run
them on stored values of any depth, including when the import brings nothing new.

Verified against code 2026-11-13: `values_eq` keeps the initial pair in a
`SmallVec` with inline capacity one. Scalars therefore compare without a heap
allocation; wider pending work can spill to the heap, and traversal remains
non-recursive. A scalar zero-allocation guard and wide-collection semantics are
covered by `crates/loro-common/tests/value_equality_allocation.rs`. Consecutive
styled insertions compare their style values when merging event hints, so a
heap-allocated initial work item used to add one allocation per merged character.

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

`encoding/value.rs::ValueReader` checks direct and nested list/map lengths before
reserving their storage. A list child needs at least one remaining kind byte; a
map entry also needs a key-index byte. In addition to the existing collection-size
cap, one counter initialized to the reader's initial raw length is debited once
for each declared child/entry and never replenished. Every successfully decoded
slot has a distinct kind byte, including across nested and successive values,
so this preserves valid compact values, duplicate keys and non-minimal varints.
Per-collection remaining-byte checks alone cannot provide that aggregate bound
because nested slices overlap. This bounds declared slots at these four sites
per reader, not exact allocator capacity/bytes, key copies, conversion storage,
other block allocations or compressed-input amplification. Failed input can be
rejected before its previous cursor/allocation/drop position; the existing
corruption error still propagates. The local reservation patch and its two new
compatibility/malformed-input tests have not yet been compiled or run.

`crates/loro/tests/deep_mark_snapshot.rs` constructs and exports marks at depths
129, 200 and 20,000 entirely on a 1 MiB stack, then reads snapshot, shallow and
state-only round trips. Each source includes both a live deep mark and a deep
mark whose text was deleted, forcing shallow redaction to decode both and
rewrite the retained live value. It also covers remote binary updates, verifies
the leaf and state/oplog frontiers, and edits/exports an unrelated map after
reads. WASM `deep-mark.test.ts` covers 129-level marks and verifies that a rejected
object JSON import still leaves its target usable.

## Equality, snapshot writers and container chains

A report against 1.17.1 found further per-level recursion reachable from untrusted
updates, each aborting a 512 KiB reader stack:

- `PartialEq for LoroValue` (and `LoroListValue`/`LoroMapValue`, which no longer
  derive it) uses a work list of value pairs in `loro-common/src/value.rs`
  (`values_eq`), short-circuiting on `Arc::ptr_eq`. Adjacent rich-text spans with
  equal but separately allocated deep marks reached it from
  `RichtextState::get_richtext_value`. NaN still equals NaN.
- Map, List and MovableList `encode_snapshot_fast` write their value with
  `value_decode::encode_map`/`encode_list`, byte-identical to the previous
  `postcard::to_io` (checked in `postcard_layout_and_remaining_metadata`).
- Container depth (`SharedArena`) is a `NonZeroU32`; it was a `NonZeroU16` whose
  `+ 1` wrapped to `None` at 65,535 levels (debug builds panicked), so
  `ContainerWrapper::new` panicked under the state lock. `ArenaContainers::get_depth`
  walks the chain with a loop, and `register_container` registers a mergeable ID's
  encoded ancestors with a loop (same registration and link order).
- `DocState::get_container_deep_value{,_with_id}` delegate to
  `state/deep_value.rs::deep_value_of`, which keeps one heap frame per open
  container. Children resolve in value order, then mergeable children, so a marker's
  child still wins its key; tree metas stay plain deep values.

Container chains are not capped at import: a fast snapshot loads its change blocks and
state lazily, so import never walks them. Readers are made iterative instead. Mergeable
names are bounded (`MAX_MERGEABLE_ANCESTOR_NAME_BYTES`, see
[mergeable-containers.md](mergeable-containers.md)) because every ancestor's name is a
prefix of the child's, so the cost is quadratic in the name, not in depth.

Building a long chain *locally* with `insert_container` is still quadratic (about 8 s
for 20,000 levels in release); import of the same chain takes about 50 ms. Tests
therefore generate deep chains as JSON updates. `crates/loro/tests/untrusted_depth.rs`
covers every case above on a 1 MiB stack.
