# Parsed Change Blocks Keep Their Content Out Of Doc-Wide Arenas

Verified against code 2026-10-16.

The doc-wide string arena (`arena/str_arena.rs:StrArena`, behind
`SharedArena::alloc_str*`) only grows. Its offsets are `u32`, and
`append-only-bytes` is built with `u32_range`. `SharedArena` used to hold a
values arena (`Vec<LoroValue>`) for list values as well, and it only grew too.

## What went wrong (zeronsh/zeron#905)

`ChangesBlock::to_bytes` (`oplog/change_store.rs`), which every export reaches
through `flush_and_compact`, turns a block into `ChangesBlockContent::Bytes`. It
drops the parsed changes. The next read parses the block again
(`ensure_changes` → `ChangesBlockBytes::parse` → `block_encode::decode_block`).
The decoder used to allocate every text insert into the string arena and every
list insert into the values arena. So each "export, then touch the block" cycle
copied the block's whole content into the arenas one more time.

Streaming into a `LoroText` with a commit and an export per tick hits this on
every tick. Consecutive commits merge into one change in the last block, and the
next commit has to parse that block to append to it. The arena grows with the
square of the streamed text: 54 KB of text left 81 MB in the arena. After about
4 GiB the `u32` offsets wrapped, and an export panicked in `get_range`
(`append-only-bytes` `lib.rs:217`, `start <= end`) with the doc locks held. List
pushes grew the values arena the same way (500 pushes, about 125,000 copies).

## The rule now

- `decode_op` (`encoding/outdated_encode_reordered.rs`) writes into a
  `DecodedBlockBuffers` that belongs to one `decode_block` call and is freed
  with the parsed changes. Text goes into one `AppendOnlyBytes`, sized by the
  block's value column. A single-value list insert is stored inline. The values
  of larger inserts are moved into one `Vec`; those ops get provisional slices,
  and `finish` points them at the finished `Arc` at the end of `decode_block`.
  A block's values are then read together, and adjacent slices merge without
  copying. The inline case matters for syncs that import one small change from
  each of many peers: each peer's change is its own small block, and a shared
  buffer per block cost about three allocations more than the old arena.
- `InnerListOp::InsertText` has no arena position (`unicode_start` is gone).
  Op merging (`list_op.rs`) uses `BytesSlice::can_merge`. Two slices that are
  adjacent in one buffer are contiguous text.
- `InnerListOp::Insert` holds a `ValueSlice` (`op.rs`) instead of a values-arena
  range (`SliceRange` is gone, and so is the values arena). A single value is
  stored inline, which is the common local insert and needs no allocation.
  Larger slices share an `Arc<Vec<LoroValue>>`. `is_mergable` only allows merges
  that copy at most one value: adjacent in one buffer, appending to a buffer the
  slice owns alone, or a single value. Merging copies only the slice, never the
  whole shared buffer.
- The trackers never address an arena. Rich-text chunks are ranges in
  `diff_calc/tracker_texts.rs:TrackerTexts`, and list chunks are ranges in
  `diff_calc/tracker_values.rs:TrackerValues` (bundled with its tracker as
  `ListTracker`). Both are per-calculator tables of the ops' own slices. They
  live and are reset with their tracker, like `styles`. Long non-ASCII op texts
  are cut into pieces, so a lookup scans at most `MAX_PIECE_BYTES`.
- A table records an insert through `Tracker::insert_with`, which builds the
  content only for the part of the op the tracker has not applied
  (`skip_applied`). A checkout that replays ops the persisted tracker already
  has used to record them again on every step, growing the tables and using up
  their `u32` index space (`persisted_walks_record_each_*_once`).
- Local text ops (`SharedArena::convert_raw_op`) and JSON text imports still
  allocate in the string arena. That cost is linear in the text written or
  imported.

## Pitfalls

- Do not reintroduce "slice an arena by an op's position". An op parsed from a
  block has no arena position.
- Do not read a `ValueSlice` that `DecodedBlockBuffers::push_values` returned
  before `finish` runs. It points into an empty placeholder. Ops are merged while
  the block is decoded, so `ValueSlice::is_mergable` refuses any merge that
  would read such a slice: only the slice before it in the same buffer may take
  it. Blocks from older versions or from loro.js can hold adjacent inserts that
  the current rules merge, such as a one-value insert (stored inline) followed by a
  larger one. That case panicked in the loro.js MovableList differential suite
  (`decode_block_with_unmerged_adjacent_list_inserts`).
- `RichOpBlockIter` (`op.rs`) starts at the first op that overlaps the span, and
  clones only the ops it returns. It used to clone every op of the change from
  the first one. That was cheap while a list insert held a `u32` range, and it
  made move-heavy movable-list diffs slow once the insert held a `ValueSlice`.
- `OpLog::is_empty` used to treat "the arenas are empty" as "nothing was
  decoded". A doc that holds only pending changes now decodes nothing into the
  arena. So `is_empty` also checks `pending_changes`. Without that check, a
  snapshot imported after a pending update replaced the op log and dropped the
  pending change. This already happened for map-only pending changes before
  this fix (`pending_map_change_survives_snapshot_import`).

## Tests and measurements

- `src/tests/text_arena_growth.rs`: the streaming workloads keep one copy of the
  text and the list values. Checkouts of re-parsed, concurrent, non-ASCII, long
  text inserts, and of list and movable-list inserts with moves and sets, match a
  fresh replay.
- `diff_calc/tracker_texts.rs` and `diff_calc/tracker_values.rs` unit tests:
  slicing across ops and pieces.
- Timing on a shared, loaded machine, best of several runs against the previous
  code. Text streaming with an export per commit: about 4x faster. B4 text
  import, checkout, and sync: the same or faster. List block decoding: about 10%
  faster. Move-heavy movable-list attach: about 25% faster (the `RichOpBlockIter`
  fix above). Local list inserts, list attach, list checkout, and two-peer list
  sync stayed within the run-to-run noise (about ±10%). A per-phase breakdown of
  list checkout showed no phase slower than before.
