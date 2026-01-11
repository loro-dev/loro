# Container State Snapshot Encoding

This document describes the binary encoding format for each container type's state snapshot. These formats are used within the `state_bytes` section of a FastSnapshot.

## Overview

Each container state is wrapped in a `ContainerWrapper` before being stored in the KV Store. The wrapper provides common metadata:

```
┌────────────────────────────────────────────────────────────────────────────┐
│                         ContainerWrapper Format                             │
├─────────────────┬──────────────────────────────────────────────────────────┤
│ 1 byte          │ ContainerType (u8, uses ContainerID.to_bytes mapping)    │
│                 │   0 = Map, 1 = List, 2 = Text, 3 = Tree, 4 = MovableList │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ LEB128          │ Depth in container hierarchy                             │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ postcard        │ Parent ContainerID (Option<ContainerID>)                 │
│                 │ WARNING: Uses historical postcard mapping (see below)    │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ variable        │ Container State Snapshot (type-specific, see below)      │
└─────────────────┴──────────────────────────────────────────────────────────┘
```

> **CRITICAL - Two Different ContainerType Mappings**:
>
> The first byte uses `ContainerID.to_bytes()` mapping, but the `Parent ContainerID`
> field uses postcard serialization which has a **different historical mapping**:
>
> | Type | First byte (to_bytes) | Postcard Serde |
> |------|----------------------|----------------|
> | Text | 2 | 0 |
> | Map | 0 | 1 |
> | List | 1 | 2 |
> | MovableList | 4 | 3 |
> | Tree | 3 | 4 |
> | Counter | 5 | 5 |
>
> When decoding `Option<ContainerID>` via postcard, use the postcard serde column.

**Source**: `crates/loro-internal/src/state/container_store/container_wrapper.rs:100-120`
**Source**: `crates/loro-common/src/lib.rs:378-401` (historical mapping)

---

## Common Encoding Patterns

### Peer ID Table

Most container states use a peer ID table to avoid repeating 8-byte peer IDs:

```
┌────────────────────────────────────────────────────────────────────────────┐
│                          Peer ID Table                                      │
├─────────────────┬──────────────────────────────────────────────────────────┤
│ LEB128          │ Number of peers (N)                                      │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ 8 × N bytes     │ Peer IDs (each u64 little-endian)                        │
└─────────────────┴──────────────────────────────────────────────────────────┘
```

### Common Columnar Structures

Many states use `serde_columnar` with these common struct patterns:

```rust
// EncodedIdFull - Full ID with lamport
#[columnar(vec, ser, de)]
struct EncodedIdFull {
    #[columnar(strategy = "DeltaRle")]
    peer_idx: usize,
    #[columnar(strategy = "DeltaRle")]
    counter: i32,
    #[columnar(strategy = "DeltaRle")]
    lamport_sub_counter: i32,  // lamport - counter (for compression)
}

// EncodedId - Compact ID with lamport only
#[columnar(vec, ser, de)]
struct EncodedId {
    #[columnar(strategy = "DeltaRle")]
    peer_idx: usize,
    #[columnar(strategy = "DeltaRle")]
    lamport: u32,
}
```

---

## MapState Snapshot

Map container stores key-value pairs with CRDT metadata.

```
┌────────────────────────────────────────────────────────────────────────────┐
│                         MapState Snapshot Format                            │
├─────────────────┬──────────────────────────────────────────────────────────┤
│ postcard        │ Map<String, LoroValue> - current visible values          │
│                 │   (HashMap encoded as varint(len) + N × (key, value))    │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ postcard        │ Vec<String> - keys with None values (deleted entries)    │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ LEB128          │ Number of peers (N)                                      │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ 8 × N bytes     │ Peer IDs (u64 little-endian each)                        │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ For each key (sorted alphabetically by key string):                        │
│   LEB128        │   peer_idx (index into peer table)                       │
│   LEB128        │   lamport timestamp                                      │
└─────────────────┴──────────────────────────────────────────────────────────┘
```

### Decoding Algorithm

```javascript
function decodeMapState(bytes) {
  // 1. Decode visible map values
  const [mapValue, rest1] = postcard.takeFromBytes(bytes);

  // 2. Decode keys with None values
  const [keysWithNone, rest2] = postcard.takeFromBytes(rest1);

  // 3. Decode peer table
  let offset = 0;
  const [peerCount, peerCountBytes] = decodeLEB128WithSize(rest2);
  offset += peerCountBytes;  // Advance past the varint

  const peers = [];
  for (let i = 0; i < peerCount; i++) {
    peers.push(readU64LE(rest2, offset));
    offset += 8;
  }

  // 4. Decode per-key metadata (from remaining bytes after peer table)
  let metaBytes = rest2.slice(offset);

  // Keys from both mapValue and keysWithNone, sorted alphabetically
  const allKeys = [...Object.keys(mapValue), ...keysWithNone].sort();

  const entries = [];
  for (const key of allKeys) {
    const [peerIdx, peerIdxBytes] = decodeLEB128WithSize(metaBytes);
    metaBytes = metaBytes.slice(peerIdxBytes);
    const [lamport, lamportBytes] = decodeLEB128WithSize(metaBytes);
    metaBytes = metaBytes.slice(lamportBytes);
    entries.push({
      key,
      value: keysWithNone.includes(key) ? null : mapValue[key],
      peer: peers[peerIdx],
      lamport
    });
  }

  return entries;
}
```

**Source**: `crates/loro-internal/src/state/map_state.rs:260-365`

---

## ListState Snapshot

List container stores ordered elements with element IDs.

```
┌────────────────────────────────────────────────────────────────────────────┐
│                         ListState Snapshot Format                           │
├─────────────────┬──────────────────────────────────────────────────────────┤
│ postcard        │ Vec<LoroValue> - list elements in order                  │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ LEB128          │ Number of peers (N)                                      │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ 8 × N bytes     │ Peer IDs (u64 little-endian each)                        │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ serde_columnar  │ EncodedListIds (columnar-encoded element IDs)            │
└─────────────────┴──────────────────────────────────────────────────────────┘
```

### EncodedListIds Structure

```
┌────────────────────────────────────────────────────────────────────────────┐
│                    EncodedListIds (serde_columnar)                          │
├─────────────────┬──────────────────────────────────────────────────────────┤
│ varint          │ Number of elements (N)                                   │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ Column 1        │ peer_idx (DeltaRle encoded usize)                        │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ Column 2        │ counter (DeltaRle encoded i32)                           │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ Column 3        │ lamport_sub_counter (DeltaRle encoded i32)               │
│                 │   Actual lamport = lamport_sub_counter + counter         │
└─────────────────┴──────────────────────────────────────────────────────────┘
```

**Source**: `crates/loro-internal/src/state/list_state.rs:529-619`

---

## RichtextState (Text) Snapshot

Richtext container stores text with optional styling marks.

```
┌────────────────────────────────────────────────────────────────────────────┐
│                       RichtextState Snapshot Format                         │
├─────────────────┬──────────────────────────────────────────────────────────┤
│ postcard        │ String - full text content                               │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ LEB128          │ Number of peers (N)                                      │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ 8 × N bytes     │ Peer IDs (u64 little-endian each)                        │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ serde_columnar  │ EncodedText (spans, keys, marks)                         │
└─────────────────┴──────────────────────────────────────────────────────────┘
```

### EncodedText Structure

```
┌────────────────────────────────────────────────────────────────────────────┐
│                     EncodedText (serde_columnar)                            │
├─────────────────┬──────────────────────────────────────────────────────────┤
│ Vec<Span>       │ EncodedTextSpan array (columnar)                         │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ Vec<String>     │ Style keys (postcard Vec<InternalString>)                │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ Vec<Mark>       │ EncodedMark array (columnar)                             │
└─────────────────┴──────────────────────────────────────────────────────────┘
```

### EncodedTextSpan (Columnar)

```
┌────────────────────────────────────────────────────────────────────────────┐
│ Column          │ Strategy       │ Description                             │
├─────────────────┼────────────────┼─────────────────────────────────────────┤
│ peer_idx        │ DeltaRle       │ Index into peer table                   │
│ counter         │ DeltaRle       │ Operation counter (i32)                 │
│ lamport_sub_counter│ DeltaRle    │ lamport - counter (i32)                 │
│ len             │ DeltaRle       │ Span length (i32, see below)            │
└─────────────────┴────────────────┴─────────────────────────────────────────┘

len interpretation:
  - positive: Text span with len Unicode characters
  - 0: Style mark start
  - -1: Style mark end
```

### EncodedMark (Columnar)

For each span with `len == 0` (style start), there's a corresponding mark:

```
┌────────────────────────────────────────────────────────────────────────────┐
│ Column          │ Strategy       │ Description                             │
├─────────────────┼────────────────┼─────────────────────────────────────────┤
│ key_idx         │ (raw varint)   │ Index into keys array                   │
│ value           │ (postcard)     │ LoroValue for the style                 │
│ info            │ (raw u8)       │ TextStyleInfoFlag byte                  │
└─────────────────┴────────────────┴─────────────────────────────────────────┘

info byte layout:
  Bit 7 (0x80): ALIVE - Style is active
  Bit 2 (0x04): EXPAND_AFTER - Expand when text inserted after
  Bit 1 (0x02): EXPAND_BEFORE - Expand when text inserted before
```

### Decoding Algorithm

```javascript
function decodeRichtextState(bytes) {
  // 1. Decode text content
  const [textContent, rest1] = postcard.takeFromBytes(bytes);

  // 2. Decode peer table
  let offset = 0;
  const peerCount = decodeLEB128(rest1);
  offset += leb128Size(peerCount);
  const peers = [];
  for (let i = 0; i < peerCount; i++) {
    peers.push(readU64LE(rest1, offset));
    offset += 8;
  }
  const columnarBytes = rest1.slice(offset);

  // 3. Decode columnar data
  const encodedText = serdeColumnar.fromBytes(columnarBytes);

  // 4. Reconstruct chunks
  const chunks = [];
  let textPos = 0;
  let markIdx = 0;

  for (const span of encodedText.spans) {
    const idFull = {
      peer: peers[span.peer_idx],
      counter: span.counter,
      lamport: span.lamport_sub_counter + span.counter
    };

    if (span.len > 0) {
      // Text chunk
      const text = textContent.slice(textPos, textPos + span.len);
      textPos += span.len;
      chunks.push({ type: 'text', id: idFull, text });
    } else if (span.len === 0) {
      // Style start
      const mark = encodedText.marks[markIdx++];
      chunks.push({
        type: 'style_start',
        id: idFull,
        key: encodedText.keys[mark.key_idx],
        value: mark.value,
        info: mark.info
      });
    } else {
      // Style end (len === -1)
      chunks.push({ type: 'style_end', id: idFull });
    }
  }

  return chunks;
}
```

> **Note on Unicode**: The `span.len` field represents Unicode scalar count (Rust's
> `unicode_len()`), not byte length or UTF-16 code units. The JavaScript example uses
> `String.slice()` which operates on UTF-16 code units, so it will produce incorrect
> results for text containing characters outside the BMP (e.g., emoji, rare CJK).
> Implementers should use proper Unicode scalar iteration (e.g., `[...str]` spread
> or `Intl.Segmenter` for grapheme-aware handling).

**Source**: `crates/loro-internal/src/state/richtext_state.rs:1130-1339`

---

## TreeState Snapshot

Tree container stores hierarchical nodes with parent relationships.

```
┌────────────────────────────────────────────────────────────────────────────┐
│                        TreeState Snapshot Format                            │
├─────────────────┬──────────────────────────────────────────────────────────┤
│ LEB128          │ Number of peers (N)                                      │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ 8 × N bytes     │ Peer IDs (u64 little-endian each)                        │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ serde_columnar  │ EncodedTree (node_ids, nodes, fractional_indexes)        │
└─────────────────┴──────────────────────────────────────────────────────────┘
```

### EncodedTree Structure

```
┌────────────────────────────────────────────────────────────────────────────┐
│                      EncodedTree (serde_columnar)                           │
├─────────────────┬──────────────────────────────────────────────────────────┤
│ Vec<NodeId>     │ EncodedTreeNodeId array (columnar)                       │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ Vec<Node>       │ EncodedTreeNode array (columnar)                         │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ bytes           │ PositionArena (fractional indexes, prefix-compressed)    │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ bytes           │ Reserved (currently empty, for future use)               │
└─────────────────┴──────────────────────────────────────────────────────────┘
```

### EncodedTreeNodeId (Columnar)

```
┌────────────────────────────────────────────────────────────────────────────┐
│ Column          │ Strategy       │ Description                             │
├─────────────────┼────────────────┼─────────────────────────────────────────┤
│ peer_idx        │ DeltaRle       │ Index into peer table                   │
│ counter         │ DeltaRle       │ Node counter (i32)                      │
└─────────────────┴────────────────┴─────────────────────────────────────────┘

TreeID = (peers[peer_idx], counter)
```

### EncodedTreeNode (Columnar)

```
┌────────────────────────────────────────────────────────────────────────────┐
│ Column              │ Strategy   │ Description                             │
├─────────────────────┼────────────┼─────────────────────────────────────────┤
│ parent_idx_plus_two │ DeltaRle   │ Parent node index + 2                   │
│                     │            │   0 = Root parent                       │
│                     │            │   1 = Deleted (tombstone parent)        │
│                     │            │   2+ = Index into node_ids + 2          │
├─────────────────────┼────────────┼─────────────────────────────────────────┤
│ last_set_peer_idx   │ DeltaRle   │ Last move operation peer index          │
│ last_set_counter    │ DeltaRle   │ Last move operation counter             │
│ last_set_lamport_sub│ DeltaRle   │ Last move lamport - counter             │
├─────────────────────┼────────────┼─────────────────────────────────────────┤
│ fractional_idx_idx  │ (raw)      │ Index into fractional_indexes           │
└─────────────────────┴────────────┴─────────────────────────────────────────┘
```

### PositionArena (Fractional Index Encoding)

Fractional indexes are prefix-compressed:

```
┌────────────────────────────────────────────────────────────────────────────┐
│                    PositionArena (serde_columnar)                           │
├─────────────────┬──────────────────────────────────────────────────────────┤
│ varint          │ Number of positions (N)                                  │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ Column 1        │ common_prefix_length (Rle encoded usize)                 │
│                 │   Bytes shared with previous position                    │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ Column 2        │ rest (raw bytes)                                         │
│                 │   Remaining bytes after common prefix                    │
└─────────────────┴──────────────────────────────────────────────────────────┘

To decode position[i]:
  position = position[i-1][0..common_prefix_length] + rest
```

**Source**: `crates/loro-internal/src/state/tree_state.rs:1556-1746`

---

## MovableListState Snapshot

Movable list allows elements to be moved while preserving their identity.

```
┌────────────────────────────────────────────────────────────────────────────┐
│                     MovableListState Snapshot Format                        │
├─────────────────┬──────────────────────────────────────────────────────────┤
│ postcard        │ Vec<LoroValue> - visible element values                  │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ LEB128          │ Number of peers (N)                                      │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ 8 × N bytes     │ Peer IDs (u64 little-endian each)                        │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ serde_columnar  │ EncodedFastSnapshot                                      │
└─────────────────┴──────────────────────────────────────────────────────────┘
```

### EncodedFastSnapshot Structure

```
┌────────────────────────────────────────────────────────────────────────────┐
│                  EncodedFastSnapshot (serde_columnar)                       │
├─────────────────┬──────────────────────────────────────────────────────────┤
│ Vec<Item>       │ EncodedItemForFastSnapshot array                         │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ Vec<IdFull>     │ list_item_ids (position IDs)                             │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ Vec<Id>         │ elem_ids (element IDs, when different from position)     │
├─────────────────┼──────────────────────────────────────────────────────────┤
│ Vec<Id>         │ last_set_ids (when different from elem_id)               │
└─────────────────┴──────────────────────────────────────────────────────────┘
```

### EncodedItemForFastSnapshot (Columnar)

```
┌────────────────────────────────────────────────────────────────────────────┐
│ Column                │ Strategy  │ Description                            │
├───────────────────────┼───────────┼────────────────────────────────────────┤
│ invisible_list_item   │ DeltaRle  │ Count of invisible items AFTER this    │
│ pos_id_eq_elem_id     │ BoolRle   │ True if position ID == element ID      │
│ elem_id_eq_last_set_id│ BoolRle   │ True if element ID == last set ID      │
└───────────────────────┴───────────┴────────────────────────────────────────┘
```

### Decoding Logic

The first item in `items` is a sentinel. For each item record:

1. **Skip first iteration** (sentinel has no visible item to decode)
2. **For non-first iterations**, consume the visible item:
   - Consume one position ID from `list_item_ids`
   - If `pos_id_eq_elem_id` is false, consume from `elem_ids`; otherwise elem_id = position_id.idlp()
   - If `elem_id_eq_last_set_id` is false, consume from `last_set_ids`; otherwise last_set_id = elem_id
   - Consume one value from the visible values list
   - Push the visible item
3. **After the visible item**, consume `invisible_list_item` invisible positions:
   - For each: consume one position ID from `list_item_ids`, push as invisible item (no value)

**Key insight**: During encoding, when an invisible item is encountered, it increments the
**previous** visible item's `invisible_list_item` counter (see line 1509). This means each
record's `invisible_list_item` represents invisible items that **follow** the visible item.

**Source**: `crates/loro-internal/src/state/movable_list_state.rs:1392-1637`

---

## CounterState Snapshot (Feature Flag)

When the `counter` feature is enabled:

```
┌────────────────────────────────────────────────────────────────────────────┐
│                       CounterState Snapshot Format                          │
├─────────────────┬──────────────────────────────────────────────────────────┤
│ 8 bytes         │ f64 value (little-endian IEEE 754)                       │
└─────────────────┴──────────────────────────────────────────────────────────┘
```

**Source**: `crates/loro-internal/src/state/counter_state.rs`

---

## LoroValue Encoding (in postcard)

LoroValue is used within container states and follows postcard enum encoding.

**Source**: `crates/loro-common/src/value.rs:714-739` (binary serde implementation)

```
┌────────────────────────────────────────────────────────────────────────────┐
│                         LoroValue Encoding                                  │
├─────────────────┬──────────────┬───────────────────────────────────────────┤
│ Variant         │ Discriminant │ Payload                                   │
├─────────────────┼──────────────┼───────────────────────────────────────────┤
│ Null            │ 0            │ (none)                                    │
│ Bool            │ 1            │ bool (0x00=false, 0x01=true)              │
│ Double          │ 2            │ f64 (8 bytes, little-endian)              │
│ I64             │ 3            │ i64 (zigzag varint)*                      │
│ String          │ 4            │ varint(len) + UTF-8 bytes                 │
│ List            │ 5            │ varint(len) + N × LoroValue               │
│ Map             │ 6            │ varint(len) + N × (String, LoroValue)     │
│ Container       │ 7            │ ContainerID encoding                      │
│ Binary          │ 8            │ varint(len) + bytes                       │
└─────────────────┴──────────────┴───────────────────────────────────────────┘

* Note: The I64 variant is historically named "I32" in the serde variant name,
  but the actual payload is always i64 encoded as zigzag varint.
```

---

## Complete Decoding Example (JavaScript)

```javascript
function decodeContainerState(bytes) {
  let offset = 0;

  // 1. Container type
  const containerType = bytes[offset++];

  // 2. Depth
  const [depth, depthBytes] = decodeLEB128(bytes.slice(offset));
  offset += depthBytes;

  // 3. Parent ContainerID (postcard Option)
  const [parent, rest] = postcard.takeFromBytes(bytes.slice(offset));
  offset = bytes.length - rest.length;

  // 4. Type-specific state
  const stateBytes = bytes.slice(offset);

  switch (containerType) {
    case 0: return { type: 'map', state: decodeMapState(stateBytes) };
    case 1: return { type: 'list', state: decodeListState(stateBytes) };
    case 2: return { type: 'text', state: decodeRichtextState(stateBytes) };
    case 3: return { type: 'tree', state: decodeTreeState(stateBytes) };
    case 4: return { type: 'movable_list', state: decodeMovableListState(stateBytes) };
    case 5: return { type: 'counter', state: decodeCounterState(stateBytes) }; // counter feature
    default: throw new Error(`Unknown container type: ${containerType}`);
  }
}
```

---

## File Locations Reference

| Container | Source File |
|-----------|-------------|
| MapState | `crates/loro-internal/src/state/map_state.rs` |
| ListState | `crates/loro-internal/src/state/list_state.rs` |
| RichtextState | `crates/loro-internal/src/state/richtext_state.rs` |
| TreeState | `crates/loro-internal/src/state/tree_state.rs` |
| MovableListState | `crates/loro-internal/src/state/movable_list_state.rs` |
| CounterState | `crates/loro-internal/src/state/counter_state.rs` |
| ContainerWrapper | `crates/loro-internal/src/state/container_store/container_wrapper.rs` |
| PositionArena | `crates/loro-internal/src/encoding/arena.rs` |
