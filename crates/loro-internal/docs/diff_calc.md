# Internal Diff Calculation

Diff calculation produces the state patch between two versions. Checkout,
forking, import, and revert all use this path.

## Replay base and changed containers

`OpLog::iter_from_lca_causally` first chooses a replay base. The DAG may return a
base older than the mathematical LCA when operations from a concurrent branch
need earlier positional context.

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
