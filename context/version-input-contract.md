# Versions and Peer Ids From Callers

Verified against code 2026-11-08.

Frontiers, version vectors, id spans and peer ids reach the public API from
callers, so they follow "invalid external input returns `Err`": no panic, and
above all no panic under the doc's locks, which poisons them for good.

## Frontiers

`Frontiers` holds one id per peer. Ops of a peer are totally ordered, so
`[1@1, 1@3]` and `[1@3, 1@3]` are the version `[1@3]`, not an invalid input. That
was already the contract: `push` keeps the greatest counter
(`test_frontiers_edge_cases`), `filter_duplicated_peer_id` does the same, and
`checkout` minimizes its input with `try_shrink_frontiers`, which also handles
non-minimal ids of different peers. The panics came from the representation:
the `From` impls built `Frontiers::Map` directly, and a map whose ids collapsed
into one entry broke the "at least 2 elements" invariant that `Frontiers::eq`
relies on. All constructors now go through `FromIterator`/`push`, and
`Frontiers::decode` inherits that. So callers normalize nothing; a caller who
wants the minimal form uses `minimize_frontiers`.

`AppDag::try_get` returns `None` for any id it does not hold, including negative
counters. `ensure_lazy_load_node` must stop for those before it treats a missing
id as a corrupt store. `VersionVector::includes_id` is already negative-safe, and
`normalize_vv_counter` treats negative ends as 0; read counters through it, or
through a span from `sub_iter`, not with raw `vv.get(..) - counter` arithmetic.

## APIs without an error in their signature

| API | Input outside the history |
|---|---|
| `frontiers_to_vv` | `None` |
| `cmp_with_frontiers` | `Less` |
| `vv_to_frontiers` | the last id of each peer, not minimized; using them reports `FrontiersNotFound` |
| `get_changed_containers_in` | the part of the range this doc holds |
| `find_id_spans_between` | deprecated; panics after releasing the locks; `try_find_id_spans_between` returns the error |
| `checkout_to_latest`, `attach` | deprecated; panic after releasing the locks; `try_checkout_to_latest` and `try_attach` return the error |

`find_id_spans_between` stays a panic because no value answers it, and an empty
diff would claim the versions are equal. Releasing the locks first keeps the doc
usable only where the panic unwinds: in a `panic = "abort"` build it still ends the
process. Code that takes versions from outside must call the `try_` form. `try_find_id_spans_between` also rejects
a version before the shallow root, as `checkout` does.

`checkout_to_latest` and `attach` have the same shape. A detached import does not
touch the state, so an update the state would reject (a list insert past the end)
gets into the op log, and checking it out fails. The `try_` forms return that
error and leave the doc detached at its version; the old forms panic after the
locks are released, and are deprecated in `loro` since 1.18.0. Exporting a snapshot
of such a doc returns the error too, where it panicked. In WASM, `attach` and
`checkoutToLatest` throw it.

`checkout` (and so these) returns `TransactionError` while a manual transaction is
open: its ops are in the DAG and not the change store, and reading that history
panicked with `DAG node must have a change`. Undo returns
`DuplicatedTransactionError` in that state for the same reason.

`minimize_frontiers` with a single id does not check that the id exists, despite
its doc comment: `try_shrink_frontiers` returns early for fewer than two ids. No
state is at risk, and `checkout` validates the result.

## Peer ids in detached editing

`check_change_greater_than_last_peer_id` rejects a transaction whose peer already
has ops after the state version. In any mode, attached too, it also rejects one
whose next id is held or awaited by parked changes (`context/import-peer-id-reuse.md`),
and the rest of this section applies the same way. A write is not the only thing that opens a
transaction: every barrier renews the auto-commit one, which `expect`ed success
under the txn lock. Commit, export, `set_peer_id` and import then panicked, and
the doc was poisoned.

`LoroDoc::auto_commit_txn` leaves the slot empty instead, because none of those
operations is what failed. `with_txn` retries with `try_start_auto_commit` and
returns `ConcurrentOpsWithSamePeerID` to the write. So:

- `set_peer_id` does not validate. It could not be the only check anyway: a
  detached doc takes in ops of its own peer on import without moving its state,
  which makes a good peer unusable later
  (`import_that_makes_the_peer_unusable_keeps_the_doc_working`).
- A doc can have no pending transaction while `can_edit()` is true. Reads, commit
  and export must not assume otherwise.
- Next-commit options held by the transaction that could not be renewed are lost.
  The next write fails anyway until the peer changes.
- `with_txn` used to return `AutoCommitNotStarted` on `wasm32` for an empty slot.
  It now starts the transaction there too, or the conflict would be misreported
  and `setPeerId` could not recover the doc.

## Text update

`DiffHandler` callbacks return nothing, so `text_update.rs` keeps the first
`LoroError` and skips the remaining edits, which were computed for a text the
failed edit did not reach. `update` returns it as `UpdateTimeoutError::Loro`,
which outranks a timeout. A diff with no edits touches nothing and returns `Ok`,
even on a deleted container.

## Tests

- `crates/loro/tests/contracts/version_frontiers.rs`: constructors, `checkout`,
  `fork_at`, and the APIs in the table.
- `crates/loro/tests/integration_test/detached_editing_test.rs`: writes, doc-level
  writes, recovery, and the import that makes a peer unusable.
- `crates/loro/tests/integration_test/text_update_test.rs`.
- `crates/loro-wasm/tests/detached_editing.test.ts`.
