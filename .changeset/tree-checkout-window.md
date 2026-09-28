---
"loro-crdt": patch
---

Fix tree checkouts that depended on the path taken. Checking out from a version to a concurrent one could keep a tree node the target version does not have, or, with a subscriber, panic with `get_index_by_tree_id(..).unwrap()` in `tree_state.rs` when the diff created a node's child without the node. The tree diff now retreats and replays from the smallest lamport that differs between the two versions instead of from a replay base, which could be a non-critical meet that skipped earlier concurrent ops. Tree checkouts also no longer search the DAG for a replay base.
