# Arena Parent Links

Verified against code 2026-10-09.

`SharedArena` (`crates/loro-internal/src/arena.rs`) stores each container's
parent. Liveness (`DocState::is_deleted`), paths (`DocState::get_path`,
`LoroDoc::get_path_to_container`), depth (`SharedArena::get_depth`, used by
diff calculation and `ContainerWrapper::new`), and event routing
(`SharedArena::with_ancestors`) all walk it.

## Where a parent link comes from

1. Parsing a change: `register_container_and_parent_link` (`parent.rs`) links
   every container an op creates to the op's container. Local ops do the same
   through `DocState::set_container_parent_by_raw_op`, and a tree `Create` diff
   through `DocState::register_meta_parents_of_created_tree_nodes`.
2. Registering a mergeable root, whose ID encodes its parent.
3. The state KV (`parent_resolver`, installed by `InnerStore::decode` when a
   snapshot is loaded): a container's own state entry records its parent.
4. The op log (`creator_resolver`, installed by `OpLog::new` from
   `ChangeStore::creator_resolver`): a normal container's ID is the ID of the
   op that created it, so loading that op's change block registers the link
   through (1).

`SharedArena::get_parent` and `get_depth` try them in that order.
`DocState::does_container_exist` (behind `get_container`, `has_container`, and
WASM `getContainerById`) calls `SharedArena::find_created_container` for a
normal ID that is not registered yet.

## Why (4) is needed

A document loaded from a snapshot, or created by `fork()`, parses its change
blocks lazily, so (1) has not run for most of the history. A full snapshot has
a state entry only for containers that were materialized. The metadata of a
tree node created inside an already deleted subtree (a concurrent child of a
deleted parent) is never materialized, so neither (1) nor (3) knew its parent,
and `is_deleted()`, edits, and events touching it panicked with "Parent is not
registered" under the state lock (loro-dev/loro#1158). Shallow snapshots are
not affected: shallow export ensures every container of the retention walk,
which includes the metadata of deleted nodes.

## Locking

The creator resolver is the only access to the change store that does not hold
the document's op log lock. `is_deleted`, `has_container`,
`get_path_to_container`, and `ContainerWrapper::new` reach it under the state
lock, and event emission (`with_ancestors`) under no document lock. So:

- `ChangeStore` takes its locks in one order, documented on the struct:
  `root_history_names`, `external_kv`, `inner`, `external_vv`, and the arena's
  lock last (parsing registers containers). Before loro-dev/loro#1159's review,
  `get_parsed_block_in`, `flush_and_compact`, and `get_change_by_lamport_lte`
  took `inner` before `external_kv`, which was harmless while the op log lock
  serialized every caller. With the resolver, `is_deleted` on one thread and
  `len_changes`, `export`, or `vv_to_frontiers` on another deadlocked.
  `ChangeStore::load_parsed_block` looks for a cached block under `inner`
  alone, then takes `external_kv` and `inner` in order and looks again before
  it loads the block from the KV store.
- The arena calls the resolver without holding its own lock.
  `ArenaContainers::get_depth` runs under the arena lock, so
  `SharedArena::get_depth` first resolves the missing links of the ancestor
  chain. It fails fast on a cycle, which would otherwise loop.
- The change store never resolves an arena parent while holding `inner`.
  `ChangeStore::visit_all_changes` runs its callback under that lock; none of
  its callers resolve parents.
- Import rollbacks run under the state lock and roll the arena back under
  `inner`; see "Import rollback" below.
- Another thread can register a container at any time without its parent, for
  example by creating a handler. Code that looks a container up twice must not
  assume the answer stayed the same: `DocState::does_container_exist` decides
  from its first lookup.

It holds weak references to the change store, so the arena does not keep the
store alive. `SharedArena::fork` drops it (it would read the source document's
op log).

## When there is no parent

The resolver answers `CreatorOp::Loaded` when a change block holds the op (it
is parsed now, so the parent of every container the op creates is registered),
`CreatorOp::Absent` when none does, and `CreatorOp::Corrupt` when a block holds
the op but cannot be parsed (see "A block that cannot be parsed"). `get_parent` returns `None` for a
normal container that no source knows:

- no op in the history creates it: an ID from the user that is not a
  container, or an op not received yet;
- a shallow document whose creating op was trimmed and whose state does not
  hold the container. Shallow export keeps everything that can be alive or
  revived, so such a container is dead at the root.

No path leads to it, so callers treat it like a deleted container:
`is_deleted` answers `true` (and does not cache it), and `get_path` and
`get_depth` answer `None`. Broken invariants still fail fast:

- A block that holds the op but cannot be decoded or parsed is reported, not
  ignored; see the next section.
- An arena without a creator resolver (not owned by an op log) panics.
- `ContainerWrapper::new` panics for a container without a parent, so a
  container never gets state without one.
- A local op's containers are linked by the transaction
  (`set_container_parent_by_raw_op`) before the change reaches the op log, so
  the resolver cannot supply a link that a local op path forgot. Debug builds
  check the links when a local change is committed
  (`parent::assert_local_parent_links_registered`).

## A block that cannot be parsed

Snapshot import validates the KV checksums (`ChangeStore::import_all`), but
loads full-snapshot history lazily. A forged or truncated block with recomputed
checksums can therefore be accepted before its contents are read. Honest data
does not produce such a block; a decode/parse failure is invalid external input,
not an impossible internal invariant.

Every ordinary history reader that fails to decode or parse a block records
it in `ChangeStore::parse_failures` (a leaf lock; the first failure is kept) and
answers "no such change", as the readers other than the resolver always did.
Legacy `Option` readers answer "no such change". The creator resolver answers
`CreatorOp::Corrupt`, which the arena treats like `Absent` for that one lookup:
`is_deleted`, `has_container`, and `get_path` cannot return an error, so the
container reads as deleted or absent and the ID as not a container. This avoids
panicking under their state lock, but does not establish that the container is
actually absent.

`OpLog::check_history_parsable` converts the record to
`DecodeError("cannot parse change block ...")`. Public fallible operations check
already recorded failures, and their history reads also propagate failures
**on the first read**, without requiring an earlier query to record them:

- `AppDag::ensure_lazy_load_node` returns `LoroResult<()>`. If the change store
  cannot provide a node, it checks the recorded parse failure and returns it.
  If there is no parse failure, an id promised by `unparsed_vv` but absent from
  the store is still an impossible internal inconsistency: the original
  "unparsed vv don't match with change store" assertion remains.
- DAG version conversion, replay-base selection, causal iteration, and diff
  calculation use fallible reads. `checkout`, `diff`, `revert_to`, `fork_at`,
  binary/JSON/batch imports, and shallow/state-only/snapshot-at exports propagate
  the error before applying state. Attached imports enable their existing
  rollback journal when the change store may still hold unparsed **bodies**,
  including register-only imports that otherwise need no journal. DAG loading
  reads headers only: draining `AppDag::unparsed_vv` does not validate bodies.
  `ChangeStoreInner::may_have_unparsed_bodies` is a conservative flag set on
  snapshot import/fork, cleared by a successful `visit_all_changes`, and set
  again when arena rollback discards parsed bodies. Individual body reads do
  not clear it; imports can therefore keep using a journal after all bodies
  have been read individually, until a full walk confirms that fact.
  The attached import, DAG journal, and change-store journal share one
  `Arc<VersionVector>` snapshot instead of copying that vector three times.
  The DAG's separate `unparsed_vv` checkpoint is still copied; after header
  warming it is empty. Initial shallow-snapshot import also
  returns the error if reading its root fails, then resets state and op log.
- `ChangeStore::try_iter_changes` validates the requested range before handing
  changes to a consumer. Export stops at an unreadable block rather than
  feeding a gap into a scratch history or returning partial bytes; the public
  export's final history check returns the recorded error. Legacy `iter_blocks`
  and `iter_changes` instead record and skip only the bad block, preserving
  the healthy portions of the requested range at their infallible boundaries.
- Query signatures that previously returned `Option` or plain values remain
  available. Use `try_frontiers_to_vv`, `try_vv_to_frontiers`,
  `try_minimize_frontiers`, `try_find_id_spans_between`, `try_get_change`
  (`OpLog::try_get_change_at`), `try_get_changed_containers_in`,
  `try_state_vv`, `try_export_json_updates`, and `try_get_cursor_pos` to
  receive decode errors. `state_vv` clones `OpLog::vv` when the state and op
  log frontiers are equal; that vector is cached and does not parse block
  bodies. Otherwise it uses `try_frontiers_to_vv`, and returns
  `FrontiersNotFound` when a frontier id is absent rather than an empty
  version vector. `cmp_frontiers` keeps its `FrontiersNotIncluded` error type,
  which can now carry the decode-error message; `travel_change_ancestors`
  returns `ChangeTravelError::HistoryUnreadable(LoroError::DecodeError(..))`.
  `ChangeTravelError` is now non-exhaustive, so downstream matches need a
  wildcard arm. `FrontiersNotIncluded` is a struct with a private field plus a
  same-named constant. Construction via the constant still compiles, but an
  exhaustive `match` of `Err(FrontiersNotIncluded)` does not (`E0004`): the
  `history_error: Some(_)` pattern cannot be written from another crate
  (`E0451`) because the field is private. Downstream must use `Err(_)`. An
  unreadable-history error is not equal to the constant.
  `CannotFindRelativePosition` is non-exhaustive and gains `HistoryUnreadable`.
  `LoroEncodeError` gains `DecodeError`, so `fork_at` keeps a decode failure
  instead of wrapping it as `Unknown`. WASM queries that already return
  `JsResult` use these fallible readers too, including `version`,
  `exportJsonUpdates`, `getCursorPos`, `findIdSpansBetween`, `frontiersToVV`,
  `vvToFrontiers`, `getChangeAt`, `getChangeAtLamport`, `getOpsInChange`, and
  `getChangedContainersIn`; their JS names and return types stay the same.

A failed fallible operation leaves materialized values, document versions, and
attached/detached status unchanged; loading valid blocks may populate history
caches. The successful DAG lazy-load path does no additional block reads or
parse-failure checks: the error-record lookup is inside the missing-node branch.
The release benchmark `crates/loro/tests/perf_history_lazy_load.rs` measures
fresh snapshot import, cold historical checkout, and concurrent update import.
Its `perf_map_import_many_peers` case measures a one-op causal map import with
1k/10k peers, both cold and after warming every DAG header without parsing old
bodies. Setup is outside the timer. Against pre-review commit `be17aed3`,
three alternating before/after pairs, each with three repeats of 30 imports,
gave median after/before ratios of 1.001/1.011 (cold) and 0.997/0.996 (headers
warmed), on macOS arm64 in release. Sharing the version snapshot removes two
extra full-vector clones from journal creation; opening the journal no longer
adds those O(peers) copies. These timings show no material increase in this
workload, rather than a bound on every import workload.

Limits and compatibility:

- Reads that need no history do not eagerly validate every block. For example,
  full snapshot import and byte-faithful full snapshot export can copy an
  unreadable block before any failure has been recorded. A query that can use
  the latest version directly may also succeed without reading it, including
  `state_vv` when the state frontiers are the op-log frontiers.
- The legacy helpers used by `undo`, `checkout_to_latest`, and `fork` retain
  their infallible boundaries. Their fallible reads record the failure first,
  then their existing unwrap boundary may panic if they need the broken history.
  A detached `fork` fails outside the source document's locks, leaving fallible
  reads on the source able to report the record. Other infallible paths can
  still unwind under locks or trap WASM. Do not add blanket record checks to
  their shared internal helpers: a recorded failure alone must not break an
  operation that can finish using healthy history. A missing node with no
  recorded parse failure is still the internal "unparsed vv don't match with
  change store" assertion in `ensure_lazy_load_node`.
- The direct cold Text comparison in `known_history.rs` falls back to the
  ordinary reader on a decoding/eligibility error. Its stricter op-length and
  change-boundary checks never record a `parse_failures` entry themselves; only
  a failure of the ordinary parser declares local history unparsable. A read
  that returns before any failure was recorded is not undone.
- Local edits and `get_deep_value` can still use the current state after a
  failure is recorded. Fallible history operations and public exports then
  reject the unreadable history, so the document can no longer be exported.

Regression tests in `change_store.rs` cover every store reader, the creator
resolver, first DAG reads on tree history, and infallible compatibility. The
internal missing-node assertion is tested in `loro_dag.rs`.
`crates/loro/tests/unregistered_container_parent.rs`
rewrites snapshot bytes through the public KV API and repairs both checksum
layers. Each fallible entry point gets its own fresh document, with the first,
middle, and last peer-1 block truncated; it must return "cannot parse change
block" without a panic, preserve the value/version/status, and leave locks
usable. It also tests initial shallow-import rollback. The regression
`register_only_import_rolls_back_when_only_old_block_headers_were_read` flips
the third byte from the end of the first/middle/last peer-1 block and repairs
the checksums. Both peers' DAG version queries succeed before a concurrent
map import first parses the bad body. The import must return the decode error,
with op log and state frontiers still equal to their original frontiers,
unchanged values/version vector, and an attached document. The healthy-doc test
`a_recorded_parse_failure_does_not_panic_where_no_error_can_be_returned` keeps
manual error records separate from actual unreadable blocks.
`crates/loro-wasm/tests/unparsable_history.test.ts` uses the same corrupted
snapshot as a fixture and checks JS exceptions, unchanged values/versions,
and a usable WASM instance after each first read. Its body-corruption fixture
also verifies a map import's rollback after both peers' header queries succeed.

## Cost of looking up an ID

`has_container` / `get_container` / `getContainerById` for a normal ID that is
not registered ask the resolver:

- No block holds the op (beyond the history, an unknown peer, before a shallow
  root): a KV scan and one block header, with no parse. About 0.5 µs per ID.
- A block holds the op: the block is parsed once and stays cached, like any
  other lazy access to that part of the history. This happens even when the op
  creates no container, because only the op's content tells. The version vector
  or DAG can tell only whether the op is in the history, not what it creates.

Measured with 2k IDs on a 20k-node, 100k-op document, `main` vs the resolver:

| IDs | `main` | resolver |
|---|---|---|
| in the history, first pass | 1.3 ms | 6.1 ms |
| in the history, second pass | 0.66 ms | 0.78 ms |
| beyond the history | 1.1 ms | 2.1 ms |

A vv shortcut for IDs beyond the history would have to be exact for every path
that writes the KV store, or a live container would read as deleted. Measured
2026-09-28 (loro-dev/loro#1159).

## Snapshot overlap decoding

`ChangeStore::decode_snapshot_for_updates` uses a temporary arena for unmatched
incoming blocks, rather than allocating a second known prefix in the document
arena. Known-history comparison resolves container IDs across the two arenas;
only checked, trimmed new ops are converted into document indices/value slices.
`register_container_and_parent_link` runs on those converted changes before they
leave the decoder. Temporary indices and parent links never escape into the
document. A rejected comparison follows the existing arena rollback path.

Cold Text-insert-only local blocks can also be compared directly from bytes
without registering anything in the document arena. Other blocks still use the
normal lazy reader. See [import-peer-id-reuse.md](import-peer-id-reuse.md).

## Import rollback

A failed import can parse old change blocks while it computes its diff
(including through the creator resolver) or validates movable-list ops
(`OpLog::resolve_movable_list_elem`), which registers containers with their
parent links and allocates their values. The arena rollback
(`SharedArena::rollback`) keeps every container index, but drops the parent
links of normal containers registered during the import and truncates the
values and text allocated during it (see
[failed-import-arena-indices.md](failed-import-arena-indices.md) for why indices
are kept). So `ChangeStore::rollback_import`, after
truncating and evicting blocks (see
[movable-list-op-validation.md](movable-list-op-validation.md)), drops the
parsed changes of the kept blocks that were parsed since the import began; the
next access parses again and registers again. `ChangesBlock::ensure_changes`
records the arena's `ArenaExtent` (containers, values, text) when it parses a
block. A block parsed before the checkpoint can only refer to what was there
then, so it keeps its parsed changes (`SharedArenaRollback::keeps`). Dropping
them all made every failed import an O(blocks) pass and the next history read a
full reparse. Blocks without bytes were built from changes inserted before the
import, so their containers were registered and linked then. Before 2026-09-28 the parsed
ops kept indices and value slices past the truncated arena: exporting the
history hit `unreachable!` in the JSON encoder, and a later checkout panicked on
a missing value (loro-dev/loro#1161).

The resolver can run during any import, because it does not take the op log
lock. So every arena rollback of a failed import goes through the change store
and runs under the state lock:

- Under `inner`: `ChangeStore::rollback_arena` rolls the arena back and drops
  the parsed changes of blocks parsed since the checkpoint. `rollback_import`
  ends with it. The
  early returns of `LoroDoc::import_changes_and_apply_delta_to_state_if_needed`
  (a decode error, or updates that depend on history before the shallow root)
  call it through `OpLog::rollback_arena`; they used to call
  `SharedArena::rollback` directly. Blocks are parsed under `inner`, so a
  block the resolver loads is either dropped by the rollback or parsed against
  the rolled-back arena. A failed snapshot import replaces the change store:
  `ChangeStore::retire` rolls the arena back and makes the old store answer
  `Absent`, so a resolver that reached it before the swap does not register the
  discarded history.
- Under the state lock: `OpLog::rollback_import`, `rollback_arena`, and
  `reset_to_empty_for_failed_snapshot_import` take `&mut DocState`, as proof
  and to clear the dead-container cache. A state-locked query (`has_container`,
  `is_deleted`, `get_path_to_container`) can register containers through the
  resolver and then walk their links before it returns; the rollback must not
  drop them in between. Without the state lock, a loom model hit `get_depth` on
  an index the rollback freed (it freed indices before loro-dev/loro#1164).

A handler created on another thread during the import keeps its index and its
container, and so do `DocState`'s store entries and caches. If the rollback
dropped the link of a container that a thread is using without the state lock,
the next lookup resolves it again; `get_path_to_container` answers `None` if
the link is gone at that moment.

## Testing pitfall

Under `cfg(test)`, `ChangeStore::import_all` parses the last change of every
peer at load. Unit tests in `loro-internal` therefore do not see lazily parsed
blocks unless the change is not the peer's last; the `loro` crate's tests run
the real path.

## Tests

- `crates/loro/tests/unregistered_container_parent.rs`: repros for every entry
  point with snapshot, fork, and shallow sources, the dumped state from the
  review harness, the import rollbacks (including #1161's missing values), a
  thread stress of queries against history readers
  (`UNREGISTERED_PARENT_THREAD_TRIALS`), and a random comparison of every tree
  meta against a full-history import (`UNREGISTERED_PARENT_SEEDS=0..300`).
- `crates/loro/tests/failed_import_state_rollback.rs`: state, handlers, and
  retries after a failed import (loro-dev/loro#1164).
- `crates/loro/tests/unregistered_container_parent.rs`
  `queries_race_with_failing_imports`: queries against imports that fail at
  decode or depend on history before a shallow root, swept across the import's
  measured duration.
- `crates/loro/tests/multi_thread_test.rs` (run by `pnpm test-loom`): the lock
  order (`resolving_a_meta_parent_while_another_thread_reads_the_history`) and
  an import that fails at decode
  (`resolving_a_meta_parent_while_an_import_fails`), as loom models.
- Unit tests in `arena.rs` (the resolver runs outside the arena lock, the
  `None` / panic / cycle cases) and `parent.rs` (the local link check).
