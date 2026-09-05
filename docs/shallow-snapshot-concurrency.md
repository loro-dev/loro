# Shallow snapshots and concurrent peers

Verified against code 2026-09-04. Test references:
`crates/loro/tests/shallow_snapshot_concurrency.rs`.

This note states what a doc bootstrapped from a `shallow-snapshot` export
guarantees when it exchanges updates with peers that still hold full history.
It is written for sync-layer authors deciding whether to upload
`shallow-snapshot` blobs in place of full snapshots.

## Vocabulary

- `A`: a full-history doc. `V < F` are versions of A; `F` is the shallow root.
- `B`: a doc bootstrapped by importing A's `shallow-snapshot` export at F.
- `C`: a peer with full history up to some version, editing on top of it.

A shallow doc retains history only since its root F. `shallow_since_vv()` /
`shallow_since_frontiers()` describe that boundary: the ops *included* by
`shallow_since_vv()` are not in the doc, and the root frontier op itself is the
first retained op.

## Guarantees

1. **Bootstrap is faithful.** B's latest state equals A's latest state, and B
   reports `is_shallow() == true` with the root at F.
   (`shallow_bootstrap_and_rereading_old_history_is_noop`)

2. **Updates causally after F apply normally.** If C synced with A at F (or
   later) and edits on top, importing C's updates into B applies them, and B
   converges with a full-history doc that saw the same updates. Importing
   post-root updates never moves B's shallow root.
   (`updates_causally_after_shallow_root_apply`)

3. **Updates causally before F are a no-op.** Re-importing history that the
   shallow root state already includes (an older `snapshot_at`, or A's full
   snapshot) returns `Ok` with nothing pending and changes nothing.
   (`shallow_bootstrap_and_rereading_old_history_is_noop`)

4. **Updates based on a version before F are rejected, not parked.** If C
   edits on top of V < F, importing those updates into B fails with
   `LoroError::ImportUpdatesThatDependsOnOutdatedVersion`. The offending
   changes are dropped: they are not applied and they do not become pending,
   so they can never be delivered to B later. B's state and shallow root are
   unchanged. The same rejection applies to a peer that never synced with A at
   all, because its genesis change has no dependencies and is treated as
   rooted before F. (`update_based_on_version_before_shallow_root_is_rejected`)
   The boundary case is included: a change whose deps are exactly the root's
   own deps — i.e. concurrent with the root frontier op itself — is rejected
   too, because its dep ids are trimmed from the DAG and no lamport can be
   computed for it. (`outdated_update_on_shallow_doc_is_dropped_not_pending`,
   which also asserts the rejected change never enters the pending store, and
   `import_deps_before_shallow_root_rejects_deps_equal_to_root_deps`)

5. **Post-root updates with missing dependencies are parked as pending.** If
   an update's dependencies are not before F but are simply not delivered yet,
   `import` returns `Ok` with `ImportStatus.pending` set. When the missing
   dependency arrives, the parked changes apply in the same call, and the
   returned `ImportStatus.success` covers both the new and the unlocked
   changes. (`pending_updates_after_root_apply_when_dependency_arrives`)

6. **Reverse direction is safe.** Updates B exports (causally after F) import
   normally into a full-history doc A′ that never saw the shallow snapshot.
   Importing B's shallow snapshot into A′ transfers the retained history (so
   A′ receives B's edits) but does not make A′ shallow: A′ keeps its full
   history and can still check out versions before F.
   (`shallow_doc_merges_back_into_full_history_doc`)

## The one lossy case

A peer whose updates are **concurrent with F** and anchored in its own
pre-root history can never merge into B
(`concurrent_chain_rooted_before_shallow_root_can_never_merge`). Importing
such an update out of order looks deceptively fine — it is parked as pending —
but the chain's genesis change is rejected under guarantee 4, so the parked
change stays pending forever and its edits never become visible.

This is inherent to shallow snapshots, not an implementation bug: applying
concurrent edits whose causal past lies before F would require changing the
state at F retroactively, which the shallow doc cannot represent. The
practical consequence for a sync layer: once you publish a shallow root at F,
every peer must sync up to some version ≥ F before its further edits can
reach shallow replicas. Peers that forked away before F and kept editing in
isolation need to rebase onto the post-root document (or the shallow replicas
will never see their work).

## Non-goals

- `checkout`, `diff`, and `revert_to` to versions before F fail with
  `SwitchToVersionBeforeShallowRoot` on a shallow doc (covered by
  `cargo test -p loro --test issue issue_928` and
  `cargo test -p loro --test contracts shallow`).
- A shallow snapshot imported into a **non-empty** doc only contributes its
  retained changes; its state sections are used only when initializing an
  empty doc.
