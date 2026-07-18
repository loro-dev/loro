# Current container-state encoding

Verified against code 2026-07-17 at commit
`fd5a1fdab79142302f0c0fbceb8807128ec6d9cd`.

This document specifies the values in the state SSTables of current ordinary
and shallow snapshots. Read it with [encoding.md](./encoding.md), which defines
the document envelope, the three snapshot sections, SSTable, postcard and
serde_columnar notation, and shallow root/overlay behavior.

The formats below are the bytes currently written by Loro. Where the current
reader accepts a broader compatibility form, that form is called out
separately; reader tolerance is not a canonical writer rule.

## 1. State SSTable entries

A state SSTable has two key classes:

| Key | Value |
|---|---|
| `ContainerID::to_bytes()` | one `ContainerWrapper` value |
| ASCII `fr` | `Frontiers::encode()`; present only in a shallow-root state |

An ordinary snapshot state and a shallow end-state overlay do not contain
`fr`. When importing a shallow snapshot, root entries are loaded first,
overlay entries replace equal keys, and the root `fr` entry is removed from the
live container store.

Writer/reader:
[`InnerStore::flush`, `decode`, `decode_twice`](../crates/loro-internal/src/state/container_store/inner_store.rs#L221-L317).
Shallow-root key:
[`FRONTIERS_KEY`](../crates/loro-internal/src/state/container_store.rs#L48-L49),
[`ContainerStore::decode_gc`](../crates/loro-internal/src/state/container_store.rs#L192-L223).

### 1.1 ContainerID state key

The state-SSTable key uses the custom `ContainerID::to_bytes()` format. It is
not postcard:

```text
root ContainerID:
    u8   container_kind | 0x80
    uleb utf8_name_len
    u8   utf8_name[utf8_name_len]

normal ContainerID:
    u8    container_kind
    u64le peer
    i32le creation_counter
```

The low-seven-bit current kind mapping is:

| Kind | Raw tag |
|---|---:|
| Map | 0 |
| List | 1 |
| Text | 2 |
| Tree | 3 |
| MovableList | 4 |
| Counter, when enabled | 5 |

Other tags decode as `Unknown(raw)`. Bit 7 is reserved as the root flag in
this particular representation, so only the low seven bits survive the key
decoder. All current known kinds are below 128.

The normal form has exactly 13 bytes. The root form must end immediately after
the declared UTF-8 name; the current reader rejects missing, extra, or invalid
name bytes.

Writer/reader:
[`ContainerID::encode`, `to_bytes`, `try_from_bytes`](../crates/loro-common/src/lib.rs#L604-L686).
Raw kind mapping:
[`ContainerType::to_u8`, `try_from_u8`](../crates/loro-common/src/lib.rs#L765-L803).

## 2. ContainerWrapper

The SSTable value boundary supplies the wrapper length. There is no inner
length before the type-specific state:

```text
u8   raw_container_kind
uleb hierarchy_depth
      postcard(Option<ContainerID>) parent
u8   type_specific_state[remaining wrapper bytes]
```

`raw_container_kind` uses the raw mapping in section 1.1 and may be an unknown
`u8`. In canonical output it agrees with the kind in the SSTable key.
`hierarchy_depth` is the arena depth recorded by the writer; it is an unsigned
LEB128, not a postcard signed integer.

Writer and lazy-byte preservation:
[`ContainerWrapper::encode`](../crates/loro-internal/src/state/container_store/container_wrapper.rs#L428-L449).
Header reader:
[`ContainerWrapper::try_new_from_bytes`](../crates/loro-internal/src/state/container_store/container_wrapper.rs#L458-L489).
Type dispatch:
[`decode_value_from_bytes`, `decode_state`](../crates/loro-internal/src/state/container_store/container_wrapper.rs#L547-L657).

### 2.1 Parent uses a second ContainerID encoding

The parent is postcard `Option<ContainerID>`, not the SSTable-key encoding.
Its exact grammar is:

```text
Option<ContainerID>:
    pvar(0)                                  # None
or
    pvar(1) postcard_container_id            # Some

postcard_container_id Root:
    pvar(0)                                  # enum variant Root
    pvar(utf8_name_len) utf8_name
    u8 historical_container_kind

postcard_container_id Normal:
    pvar(1)                                  # enum variant Normal
    pvar(u64 peer)
    pvar(i32 creation_counter)               # zigzag
    u8 historical_container_kind
```

For these small option/variant values, `pvar(0)` and `pvar(1)` are bytes `00`
and `01`. Postcard structs and struct variants do not add field names or a
field count.

The historical kind mapping used only inside postcard differs from the raw
mapping:

| Kind | Historical postcard tag |
|---|---:|
| Text | 0 |
| Map | 1 |
| List | 2 |
| MovableList | 3 |
| Tree | 4 |
| Counter, when enabled | 5 |

Other bytes become `Unknown(byte)`. A build without the counter feature also
treats historical/raw tag 5 as unknown. The public `loro` crate enables the
counter feature by default.

ContainerID fields and derived postcard order:
[`ContainerID`](../crates/loro-common/src/lib.rs#L589-L602).
Historical kind serializer:
[`historical_container_type_to_byte`, `ContainerType::serialize`](../crates/loro-common/src/lib.rs#L805-L903).
Default feature:
[`crates/loro/Cargo.toml`](../crates/loro/Cargo.toml#L37-L42).

### 2.2 Shared peer table

List, Text, Tree, and MovableList states, plus Map metadata, use:

```text
uleb peer_count
repeat peer_count times:
    u64le peer
```

Indices are zero-based. The canonical writer registers peers on first use in
the container-specific traversal described below. Readers must reject an index
outside the table. This table is deliberately fixed-width little-endian even
though a peer inside postcard `ContainerID` is a varint.

Shared reader and bounds checks:
[`decode_peer_table`, `decode_peer_from_table`](../crates/loro-internal/src/state.rs#L97-L129).

### 2.3 serde_columnar reminder

The complete strategy grammar is in
[encoding.md section 8](./encoding.md#8-serde_columnar-grammar-used-below). For
each top-level state struct below:

```text
pvar(outer_field_count)
each ordinary field in order, or for class="vec":
    pvar(nested_column_count)
    repeat nested_column_count times:
        pvar(column_payload_byte_len)
        u8 column_payload[column_payload_byte_len]
```

Thus `outer=1, nested=3` means two distinct leading counts; it never means a
single leading `3`. Strategy column payloads infer their row count. A Generic
column payload is postcard `Vec<T>`, so it starts with its own row count.

## 3. Postcard LoroValue used by states

Map, List, Text marks, and MovableList use postcard serialization of
`LoroValue`. This is different from the custom operation value codec in the
updates format.

| Variant | Enum tag | Payload |
|---|---:|---|
| Null | 0 | none |
| Bool | 1 | postcard bool `00` or `01` |
| Double | 2 | IEEE-754 `f64le` |
| I64 | 3 | postcard zigzag `i64` |
| String | 4 | `pvar(utf8_len) + UTF-8` |
| List | 5 | `pvar(item_count)` then values |
| Map | 6 | `pvar(entry_count)` then String/value entries |
| Container | 7 | postcard `ContainerID` from section 2.1, without an Option |
| Binary | 8 | `pvar(byte_len) + bytes` |

The I64 variant is historically named `I32` in serde metadata, but its payload
is `i64`. Map entry order is not semantic and is not canonical because the
underlying map is hash-based. A decoder must not attach meaning to its order.

Binary serde implementation:
[`LoroValue::serialize`, `deserialize`](../crates/loro-common/src/value.rs#L719-L795).
ContainerID and historical type source: section 2.1 above.

## 4. Map state

The type-specific bytes are:

```text
postcard(FxHashMap<String, LoroValue>) visible_values
postcard(Vec<InternalString>)          keys_with_none_value
shared_peer_table
repeat once for every distinct key in the union, sorted as described below:
    uleb peer_index
    uleb lamport_u32
EOF
```

`visible_values` contains keys whose current register has `Some(value)`.
`keys_with_none_value` contains tombstoned registers. Canonical output has no
duplicate tombstoned key and no key in both collections. The hash-map and
tombstone-vector iteration orders are not semantic. The metadata rows are the
only ordered-by-key portion.

For metadata, take the union of the two key collections and sort using Rust
`str::cmp`: lexicographic UTF-8 byte order, which is also Unicode scalar-value
order. Do not use JavaScript's default `Array.sort()`, which compares UTF-16
code units. For example, U+10000 sorts after U+E000 in Rust but before it under
the JavaScript default. Each row supplies the last-writer `(peer, lamport)` for
the corresponding sorted key.

The canonical peer table is first-use order while iterating the internal map,
before the sorted metadata pass. Its order, like the hash-map entries, is not a
stable byte-canonicalization promise. A reader reconstructs the sorted key
union, checks every peer index and `u32` Lamport conversion, and requires exact
EOF after the last metadata row.

Writer and ordering:
[`MapState::encode_snapshot_fast`](../crates/loro-internal/src/state/map_state.rs#L492-L531).
Reader and validation:
[`MapState::decode_snapshot_fast`](../crates/loro-internal/src/state/map_state.rs#L536-L607).
String ordering:
[`InternalString::Ord`](../crates/loro-common/src/internal_string.rs#L43-L60).

Mergeable-container activation markers, when present, are ordinary Binary
`LoroValue` payloads at this layer; their higher-level interpretation is
specified in
[`mergeable-container-id.md`](../crates/loro-internal/docs/mergeable-container-id.md).

## 5. List state

```text
postcard(Vec<LoroValue>) visible_values
shared_peer_table
columnar(EncodedListIds)
```

`EncodedListIds` has outer field count 1. Its only field is a class vector with
three nested columns:

```text
pvar(1)                              # EncodedListIds fields
pvar(3)                              # EncodedListId columns
bytes DeltaRle<usize> peer_index
bytes DeltaRle<i32>   counter
bytes DeltaRle<i32>   lamport_sub_counter
```

Row `i` identifies visible value `i`:

```text
peer    = peers[peer_index]
lamport = counter + lamport_sub_counter
```

The canonical peer table is first use in visible list order. The number of ID
rows must equal `visible_values.len()`. The current reader rejects an invalid
peer index, a negative counter, a negative/overflowing Lamport result, and an
ID/value length mismatch.

Writer, row structs, and reader:
[`EncodedListId`, `EncodedListIds`, `ListState` snapshot codec](../crates/loro-internal/src/state/list_state.rs#L759-L858).
Shared ID arithmetic checks:
[`decode_lamport_from_delta`](../crates/loro-internal/src/state.rs#L144-L160).

## 6. Text state

```text
postcard(String) full_text
shared_peer_table
columnar(EncodedText)
```

The full text is UTF-8 in postcard. All lengths in span metadata count Unicode
scalar values, not UTF-8 bytes, UTF-16 code units, or grapheme clusters.

### 6.1 EncodedText layout

`EncodedText` has three outer fields:

```text
pvar(3)                              # EncodedText fields

# field 0: class vector spans
pvar(4)                              # EncodedTextSpan columns
bytes DeltaRle<usize> peer_index
bytes DeltaRle<i32>   counter
bytes DeltaRle<i32>   lamport_sub_counter
bytes DeltaRle<i32>   len

# field 1: ordinary postcard Vec<InternalString>
pvar(style_key_count)
repeat style_key_count times:
    pvar(utf8_len) utf8_style_key

# field 2: ordinary postcard Vec<EncodedMark>
pvar(mark_count)
repeat mark_count times:
    pvar(3)                          # EncodedMark row field count
    pvar(usize key_index)
    postcard(LoroValue) value
    u8 info
```

Canonical type-specific state ends after this complete columnar object. The
current serde_columnar 0.3.14 `from_bytes` and `iter_from_bytes` entrypoints do
not expose or check the decoder remainder. Consequently, the current List,
Text, Tree, and MovableList readers ignore trailing bytes after an otherwise
valid columnar object. This is reader tolerance, not extension space for a
writer; a strict decoder should require exact consumption.

Dependency source:
`serde_columnar-0.3.14/src/lib.rs::{from_bytes,iter_from_bytes}` lines 122-137
at the revision pinned in encoding.md. In-repository call sites are the reader
links in sections 5 through 8.

The `spans` field is columnar. `keys` and `marks` are ordinary row-wise
postcard vectors even though `EncodedMark` has generated columnar row support.
The canonical peer table is first use during the single
`RichtextStateChunk` traversal that emits spans: Text, StyleStart, and StyleEnd
chunks all register their peer in exact chunk order. Style keys are registered
on first StyleStart occurrence. There is exactly one mark row per start span,
in start-span order.

Schema and writer:
[`EncodedTextSpan`, `EncodedMark`, `EncodedText`, `encode_snapshot_fast`](../crates/loro-internal/src/state/richtext_state.rs#L1219-L1341).

### 6.2 Span semantics and validation

For every span:

```text
peer    = peers[peer_index]
lamport = counter + lamport_sub_counter
id      = (peer, counter, lamport)
```

`len` has exactly three valid classes:

| `len` | Meaning |
|---:|---|
| `> 0` | consume this many Unicode scalar values from `full_text` |
| `0` | style start; consume the next mark row |
| `-1` | style end; match the start at `(peer, counter - 1)` |

Values below `-1` are invalid. The canonical writer represents an end using
the start's peer, `counter = start.counter + 1`, and the same
`lamport_sub_counter`; consequently the reconstructed end Lamport is also the
start Lamport plus one. The end reuses the original `StyleOp`; it does not
carry another mark row.

A compatible reader must also verify:

- peer and style-key indices are in range;
- scalar spans consume the full text exactly;
- every start has one mark and every mark is consumed;
- every end matches a live start and no start remains unclosed; and
- counter/Lamport arithmetic is valid.

Reader:
[`RichtextState::decode_snapshot_fast`](../crates/loro-internal/src/state/richtext_state.rs#L1354-L1468).

`info` is preserved as one raw byte. Currently assigned bits are `0x80` alive,
`0x02` expand-before, and `0x04` expand-after.

Flag source:
[`TextStyleInfoFlag`](../crates/loro-internal/src/container/richtext.rs#L135-L166).

## 7. Tree state

Tree has no postcard visible-value prefix. Its complete type-specific bytes
are:

```text
shared_peer_table
columnar(EncodedTree)
```

`ContainerWrapper` special-cases Tree: decoding the state constructs its
derived public list value. Treating the first tree byte as a postcard value tag
will misalign the stream.

Tree dispatch:
[`ContainerWrapper::decode_value_from_bytes`](../crates/loro-internal/src/state/container_store/container_wrapper.rs#L547-L612).

### 7.1 EncodedTree layout

```text
pvar(4)                              # EncodedTree fields

# field 0: node_ids class vector
pvar(2)
bytes DeltaRle<usize> node_peer_index
bytes DeltaRle<i32>   node_counter

# field 1: nodes class vector
pvar(5)
bytes DeltaRle<usize> parent_index_plus_two
bytes DeltaRle<usize> last_set_peer_index
bytes DeltaRle<i32>   last_set_counter
bytes DeltaRle<i32>   last_set_lamport_sub_counter
bytes Generic<usize>  fractional_index_index

# field 2: Cow<[u8]>
bytes position_arena_bytes

# field 3: Cow<[u8]>
bytes reserved_has_effect_bool_rle   # canonical writer: byte length 0
```

`Generic<usize>` contains postcard `Vec<usize>`: its column payload begins
with the row count, then unsigned postcard varints. The reserved field is an
empty byte string, so it still contributes its `pvar(0)` length byte to the
outer struct. The current Tree reader never reads or validates the decoded
reserved field, so it also accepts arbitrary nonempty bytes inside that
length-delimited field; canonical writers must emit it empty.

The current writer orders all alive nodes in breadth-first order, followed by
all deleted nodes in breadth-first order. `node_ids.len()` equals
`nodes.len()`. Node row `i` describes node ID row `i`.

Canonical peer registration is intentionally not interleaved by row. The
writer first registers every node-ID peer in alive-BFS then deleted-BFS order.
Only after that complete pass does it register last-set peers: first all alive
node rows, then all deleted node rows.

Node schema and ordering:
[`EncodedTreeNodeId`, `EncodedTreeNode`, `EncodedTree`](../crates/loro-internal/src/state/tree_state.rs#L1556-L1590),
[`encode`, `TreeState::encode_snapshot_fast`](../crates/loro-internal/src/state/tree_state.rs#L1591-L1690).

### 7.2 Node row semantics

```text
node_id = (peers[node_peer_index], node_counter)

parent_index_plus_two = 0  => root
parent_index_plus_two = 1  => deleted root
parent_index_plus_two >= 2 => node_ids[parent_index_plus_two - 2]

last_set_id = (
    peers[last_set_peer_index],
    last_set_counter,
    last_set_counter + last_set_lamport_sub_counter
)

fractional_index = positions[fractional_index_index]
```

The reader checks both peer indices, parent and position indices, nonnegative
node counters, Lamport arithmetic, equal node-vector lengths, and errors from
inserting an invalid node.

Reader:
[`TreeState::decode_snapshot_fast`](../crates/loro-internal/src/state/tree_state.rs#L1695-L1773).

### 7.3 PositionArena in tree state

The writer collects every node fractional index in a `BTreeSet`, producing a
unique table in lexicographic raw-byte order (`FractionalIndex` derives `Ord`
from its `Vec<u8>`). It then writes `PositionArena::encode()` even when the
table is empty; unlike mode-4 change blocks, an empty tree position arena is
not represented by zero inner bytes.

`position_arena_bytes` contains:

```text
pvar(1)                              # PositionArena fields
pvar(2)                              # PositionDelta columns
bytes Rle<usize> common_prefix_length
bytes Generic<Cow<[u8]>> rest
```

The Generic `rest` payload is postcard `Vec<Cow<[u8]>>`:

```text
pvar(row_count)
repeat row_count times:
    pvar(rest_byte_len) rest_bytes
```

Row 0 must have common prefix zero. Later row `i` is reconstructed from
`position[i-1][0..common_prefix_length[i]] + rest[i]`; a prefix beyond the
previous byte length is invalid.

Position collection:
[`tree_state.rs::encode`](../crates/loro-internal/src/state/tree_state.rs#L1591-L1664).
Position ordering:
[`FractionalIndex`](../crates/fractional_index/src/lib.rs#L15-L18).
Arena schema, writer, and reader:
[`PositionDelta`, `PositionArena`](../crates/loro-internal/src/encoding/arena.rs#L159-L252).

## 8. MovableList state

```text
postcard(Vec<LoroValue>) visible_values
shared_peer_table
columnar(EncodedFastSnapshot)
```

`EncodedFastSnapshot` has four class-vector fields:

```text
pvar(4)                              # outer fields

# field 0: items
pvar(3)
bytes DeltaRle<usize> invisible_list_item
bytes BoolRle         pos_id_eq_elem_id
bytes BoolRle         elem_id_eq_last_set_id

# field 1: list_item_ids
pvar(3)
bytes DeltaRle<usize> peer_index
bytes DeltaRle<i32>   counter
bytes DeltaRle<i32>   lamport_sub_counter

# field 2: elem_ids
pvar(2)
bytes DeltaRle<usize> peer_index
bytes DeltaRle<u32>   lamport

# field 3: last_set_ids
pvar(2)
bytes DeltaRle<usize> peer_index
bytes DeltaRle<u32>   lamport
```

Schema:
[`EncodedId`, `EncodedItemForFastSnapshot`, `EncodedIdFull`, `EncodedFastSnapshot`](../crates/loro-internal/src/state/movable_list_state.rs#L1389-L1430).

### 8.1 Sentinel and stream consumption

`items[0]` is mandatory and has no visible value or visible item ID. Its two
boolean flags are canonically true. Its `invisible_list_item` count represents
leading tombstoned list positions before the first visible item.

For each later `items` row, in order:

1. consume one `list_item_ids` row and one `visible_values` item;
2. if `pos_id_eq_elem_id` is true, derive the element ID from that full list
   item ID; otherwise consume one `elem_ids` row;
3. if `elem_id_eq_last_set_id` is true, reuse the element ID; otherwise consume
   one `last_set_ids` row; and
4. after that visible item, consume `invisible_list_item` more
   `list_item_ids` rows as tombstoned positions.

Thus `items.len() == visible_values.len() + 1`. The sentinel count handles
leading invisible positions; every later count handles invisible positions
after its visible item and before the next visible item. `list_item_ids`
contains one row for every underlying visible or invisible list position.

An `EncodedIdFull` reconstructs
`(peers[peer_index], counter, counter + lamport_sub_counter)`. An `EncodedId`
reconstructs `(peers[peer_index], lamport)`.

The canonical peer table is first use during the underlying list traversal.
The current reader requires the sentinel and rejects missing or extra values,
list-item IDs, element IDs, and last-set IDs, as well as invalid peer indices
or ID arithmetic.

Writer:
[`MovableListState::encode_snapshot_fast`](../crates/loro-internal/src/state/movable_list_state.rs#L1464-L1531).
Reader and exact stream-consumption checks:
[`MovableListState::decode_snapshot_fast`](../crates/loro-internal/src/state/movable_list_state.rs#L1544-L1766).

## 9. Counter state

With the counter feature enabled, the canonical type-specific state is exactly:

```text
f64le counter_value
```

There is no separate metadata suffix. The canonical writer always emits eight
bytes. For compatibility, the current reader also accepts zero remaining bytes
and interprets them as `0.0`; this permits a build without counter support to
re-export an untouched header-only unknown counter. Any other byte length is
rejected by the counter reader.

Writer and compatibility reader:
[`CounterState` snapshot codec](../crates/loro-internal/src/state/counter_state.rs#L93-L131).

## 10. Unknown state

Any raw wrapper kind not known in the current feature build becomes
`ContainerType::Unknown(raw)`. There are two distinct preservation cases:

- A lazy `ContainerWrapper` re-encodes its entire original wrapper value
  verbatim, so an ordinary snapshot can preserve unknown payload bytes without
  understanding them.
- If the wrapper is materialized as `UnknownState`, its public value is Null,
  the decoder does not interpret the remaining payload, and the current
  `UnknownState` encoder emits no type-specific bytes. Re-encoding after that
  point can therefore discard an opaque payload.

The current shallow-snapshot writer's unknown handling is path-dependent. A
generic root rebuild rejects unknown materialized live containers. Reusing a
cached root for an overlay rejects unknown retained root keys. However, the
cached-root replay-only `E` fast path returns the existing root bytes without
that check, so a lazy root unknown can survive. Containers introduced after
the root are not checked again: a post-root unknown can be carried by retained
operations with `E`, or its raw/lazy wrapper bytes can be selected into an
overlay SSTable. If a wrapper is materialized as `UnknownState` before
re-encoding, the opaque type-specific payload can still be lost as described
above. Unknown support is forward-preservation behavior, not permission to
interpret its bytes as a known state codec.

Lazy preservation and dispatch:
[`ContainerWrapper::encode`, `decode_value_from_bytes`](../crates/loro-internal/src/state/container_store/container_wrapper.rs#L428-L449),
[`container_wrapper.rs`](../crates/loro-internal/src/state/container_store/container_wrapper.rs#L547-L657).
Unknown codec:
[`UnknownState` snapshot codec](../crates/loro-internal/src/state/unknown_state.rs#L83-L99).
Shallow fast path and checks:
[`export_shallow_snapshot_inner`](../crates/loro-internal/src/encoding/shallow_snapshot.rs#L68-L169),
[`has_unknown_container`, `has_unknown_container_key`](../crates/loro-internal/src/encoding/shallow_snapshot.rs#L189-L203).

## 11. Canonical writer and reader checklist

A compatible implementation should keep these independent invariants:

1. Parse the SSTable key with custom `ContainerID::to_bytes()` rules, but parse
   wrapper parents and state `LoroValue::Container` with postcard ContainerID.
2. Use the raw kind mapping for the state key/wrapper and the historical kind
   mapping only inside postcard ContainerID.
3. Bound every wrapper by its SSTable value; there is no inner state length.
4. Read both the outer struct count and nested column count for every
   serde_columnar class vector.
5. Treat postcard maps as unordered, while matching Map metadata rows to the
   Rust scalar/UTF-8-sorted union of keys.
6. Reconstruct Lamport values with checked arithmetic and validate every peer,
   key, parent, and position index.
7. Count Text spans in Unicode scalar values, pair style ends with the preceding
   style ID, and consume text and marks exactly.
8. Keep Tree's absent value prefix, alive-then-deleted node order, sorted unique
   positions, and non-empty serialization envelope for an empty PositionArena.
9. Require MovableList's sentinel and exhaust all four metadata streams and the
   visible-value stream.
10. Emit Counter as eight little-endian bytes; regard its empty-reader form as
    compatibility input only.
11. Preserve unknown wrapper bytes lazily or reject them explicitly; do not
    silently reinterpret or normalize them.

The concrete validation paths are the reader links in sections 4 through 10.
