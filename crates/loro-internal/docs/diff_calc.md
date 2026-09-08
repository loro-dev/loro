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

Anything else (a text, list, movable list, tree or unknown container with ops
on both sides) falls back to the DAG's conservative answer exactly as before.
Shallow docs need no special case: trimmed history is causally before every
retained change (`import_deps_before_shallow_root`), so it can never be
concurrent with the new region.

Map diffs in `Checkout`/`Import` mode only look up the keys written inside the
replayed span, and skip replayed ops that both versions already contain, so
their cost follows the update instead of the map size.

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
