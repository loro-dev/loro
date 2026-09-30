# Context Discoverability Gaps (backlog)

Append a line when you discovered something important the hard way but could not
fix the docs in that change.

Format:
`YYYY-MM-DD | <question an agent would ask> | <answer + file anchors> | why it was hard | suggested home`
2026-09-30 | Does `fuzz::actor::assert_value_eq` / `Actor::check_tracker` fail on a mismatch? | No: since #953 it only `tracing::warn!`s (`crates/fuzz/src/actor.rs`), so tracker and value checks in the fuzz crate (including `crates/fuzz/tests/checkout_path.rs`, which `context/tree-checkout-window.md` describes as comparing values) fail only when the tracker panics | a randomized repro of loro-dev/loro#1157 passed on the old code | `context/tree-checkout-window.md` Tests section, or make it assert
