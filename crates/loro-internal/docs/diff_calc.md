# Internal Diff Calculation

Diff calculation produces the state patch between two versions. Checkout,
forking, import, and revert all use this path.

## Replay base and changed containers

`OpLog::iter_from_replay_base_causally` first chooses a replay base. The safe
base is a **critical version** in the Eg-walker sense (arXiv:2409.14252 §3.5):
a version that no concurrency crosses. When a concurrent branch invalidates
the candidate, the DAG retreats the base to the latest single-head critical
version, which can be older than the meet of the two versions.

An old base is not evidence that every container in the replay range changed.
`DiffCalculator::calc_diff_internal` derives the changed container set from the
version-vector difference and routes common history only to those calculators.
Otherwise each unchanged List/Text/MovableList can trigger its own full-history
tracker rebuild.

The DAG walk follows explicit dependencies plus the implicit previous counter
of the same peer. An implicit path may be redundant when an explicit relay
dependency already contains that predecessor. The walk remembers the dependency
tip where an unmatched path split and performs a targeted ancestor lookup from
the candidate common frontiers:

- a covered tip is another route into already-common history, so `from` remains
  the replay base;
- an uncovered tip is a real concurrent branch, so the conservative base is
  retained.

This lookup runs only for unmatched branch tips and prunes by visited DAG nodes
and Lamport time. It does not calculate a causal version for every new peer and
does not compare each one with the complete `from` version vector.

## Register-only concurrency

`to ⊇ from` does not imply the `ImportGreaterUpdates` contract: the new ops
may be concurrent with part of `from` (a stale unmerged head is the common
shape). The DAG alone can only demote such an import to `Checkout` and
retreat the base to a critical version, which replays the whole branch and
makes Text/List rebuild trackers from empty on every import.

`OpLog::iter_from_replay_base_causally` therefore checks the concurrency at
container granularity before consulting the DAG:

1. `OpLog::uncovered_entry_parents` finds the entry changes of the new region
   whose causal parents do not cover all of `from` (the version-vector form
   of spec lemma L12; it scans only the new changes). Old history outside
   `⋂ Events(parents)` may be concurrent with a new op; everything else is
   causally before the whole new region.
2. `OpLog::register_only_concurrency` scans the ops in that concurrent old
   history and in the new region. If every container present on both sides
   is a register (Map, Counter), the import replays from `from` in
   `ImportGreaterUpdates` mode.

Registers are harmless because their diffs need no positional context, but the
overlapping maps must still be resolved from the history cache (the calculator
is started in `Import` mode for them): persisted state drops the lamport
metadata of deleted roots and dead containers, so comparing lamports against
the state is not sound for concurrent ops. Containers with no concurrent old
ops keep the fast path; they are not marked `source_not_in_op_context`.

On a shallow doc the history cache seeds a map's shallow-root entries only
when that map is first resolved (`ContainerHistoryCache::ensure_shallow_map_seeded`),
so the first concurrent map import costs O(that map), not O(every map in the
shallow root). Regression and perf test: `crates/loro/tests/shallow_lazy_map_checkout_index.rs`.

Anything else (a text, list, movable list, tree or unknown container with ops
on both sides) falls back to the DAG's conservative answer exactly as before.
So does a shallow doc whose concurrent old history reaches below the shallow
root: later imports must causally follow the root
(`import_deps_before_shallow_root`), but the snapshot itself can retain
changes concurrent with the root (independent peer chains), and the trimmed
ops in `from` cannot be scanned for containers. The regression test is
`shallow_doc_accepts_cross_peer_op_whose_deps_include_boundary`.

Map diffs in `Checkout`/`Import` mode only look up the keys written inside the
replayed span, and skip replayed ops that both versions already contain, so
their cost follows the update instead of the map size.

## Winner metadata, not just values

Map and MovableList states store the winning op's lamport/peer (map entry,
movable-list `value_id`) next to the value. A checkout must move that metadata
even when both versions hold an equal value written by different ops (a value
rewritten after the target, e.g. by `revert_to`).

`MapDiffCalculator` (Checkout/Import) compares the winners at `from` and `to`
by op id only (`MapHistoryCache::changed_winners_for_keys`): the same winner is
skipped without reading any value, and a different winner is emitted with the
`to` value. The `from` value is never fetched, because the state applying the
diff is already at `from` (the isolated-import fast path diffs from the empty
version, where every winner is new). `MapState` writes each entry with one
`insert` and reports a change only when the previous value differs, so an
equal-value entry is a silent metadata update and events still only report
value changes. `MovableListState` does the same for element values. This keeps
equal-value checkouts at the cost of `main` before the fix: the added state
write per key is paid for by the dropped `from` value lookup (loro-dev/loro#1124;
benchmark: `crates/loro/examples/map_equal_value_bench.rs`).

Skipping those entries left the later op's metadata in the checked-out state.
Shallow and state-only exports build their root state through such a checkout,
so the root state carried a lamport/peer that is not in the root version. On
import, `ensure_shallow_map_seeded` inserted that entry into the map checkout
index, where it compared equal to the retained later op (the index is keyed by
container, key, lamport and peer) and was dropped, so `checkout(root)` lost the
key whenever its root-time writer was trimmed. `get_last_editor` after a
checkout was wrong for the same reason. Regression tests:
`crates/loro/tests/shallow_checkout_equal_value.rs`.

## Diff modes

- `Checkout` is the general and slowest mode. It can move in either direction
  and may use `ContainerHistoryCache`.
- `Import` requires `to > from`, but some imported operations may be concurrent
  with `from`.
- `ImportGreaterUpdates` additionally guarantees that every imported operation
  is causally after `from`, so the replay base is `from`.
- `Linear` additionally guarantees that imported operations are ordered, so
  diff calculation does not need to build CRDT trackers.

List, Text, and MovableList still rebuild their trackers from CRDT IDs when
retreating, when their source context is incomplete, or when shallow history
requires it. That fallback is a correctness requirement, not a replay-base
optimization.
