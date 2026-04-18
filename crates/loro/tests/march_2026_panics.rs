//! Regression tests for the March-2026 panic reports captured in `loro-debug/`.
//!
//! Each test loads a production blob from `tests/fixtures_march_2026/` and
//! exercises the code path (import / importBatch / forkAt) that was observed
//! to panic in `loro-crdt@1.10.6`. The tests must not panic and — where an
//! invariant violation is known to be on the client side — must return a
//! proper `LoroError` instead.

use loro::{LoroDoc, VersionVector};

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

// ---------------------------------------------------------------------------
// Bug #1: DagCausalIter stack ordering (kim-bootstrap / kim-live-update /
// platform-01knrqby / platform-fork-at). A real DAG bug: when importing an
// update whose start-VV exactly matches the current doc's VV on some peer but
// the update's start-frontiers reference a *different* peer, the causal iter
// builds a stack containing a node whose counter does not equal the current
// span min, violating its invariant.
// ---------------------------------------------------------------------------

#[test]
fn kim_bootstrap_does_not_panic() {
    let initial = include_bytes!("fixtures_march_2026/kim_bootstrap__initial.bin");
    let op1 = include_bytes!("fixtures_march_2026/kim_bootstrap__op1.bin");

    let doc = LoroDoc::new();
    doc.import(initial).expect("initial snapshot imports");
    doc.import(op1).expect("bootstrap update imports without panic");
}

#[test]
fn kim_live_update_does_not_panic() {
    let initial = include_bytes!("fixtures_march_2026/kim_live_update__initial.bin");
    let op1 = include_bytes!("fixtures_march_2026/kim_live_update__op1.bin");
    let op2 = include_bytes!("fixtures_march_2026/kim_live_update__op2.bin");

    let doc = LoroDoc::new();
    doc.import(initial).expect("initial snapshot imports");
    doc.import(op1).expect("live update 1 imports");
    doc.import(op2).expect("live update 2 imports without panic");
}

#[test]
fn platform_import_does_not_panic() {
    let initial =
        include_bytes!("fixtures_march_2026/platform_01knrqby0mdk5e85bgcyefzx87__initial.bin");
    let op1 = include_bytes!("fixtures_march_2026/platform_01knrqby0mdk5e85bgcyefzx87__op1.bin");

    let doc = LoroDoc::new();
    doc.import(initial).expect("initial imports");
    doc.import(op1).expect("platform update imports without panic");
}

#[test]
fn platform_fork_at_does_not_panic() {
    let initial = include_bytes!(
        "fixtures_march_2026/platform_fork_at_01knrqby0mdk5e85bgcyefzx87__initial.bin"
    );
    let vv_bytes = include_bytes!(
        "fixtures_march_2026/platform_fork_at_01knrqby0mdk5e85bgcyefzx87__forkAt_vv.bin"
    );

    let doc = LoroDoc::new();
    doc.import(initial).expect("initial snapshot imports");

    let vv = VersionVector::decode(vv_bytes).expect("decode VV");
    let frontiers = doc.vv_to_frontiers(&vv);
    // forkAt previously panicked in DagCausalIter with the same assertion.
    let forked = doc.fork_at(&frontiers).expect("fork_at should not panic");
    drop(forked);
}

// ---------------------------------------------------------------------------
// Bug #2: list diff calculator produces a malformed delta (`Retain(N)` where
// `N` overshoots the receiving list's length). Historically this reached
// `LoroList::insert` and panicked with "Index 8 out of range. The length is
// 5", leaking out of `doc.import`. The snapshot + update pair is
// self-consistent at the change-DAG level (every change's deps are
// satisfiable within the blob), so this is a Loro-side diff/apply
// inconsistency — rooted in the `RichtextTracker` cold-start path used by
// `ListDiffCalculator` after a snapshot import.
//
// The new expectation: `doc.import` returns an `Err` surfaced from
// `ContainerState::apply_diff`, not a panic. The oplog has already accepted
// the changes at this point, but the state layer rejected the diff as
// malformed and the caller can observe the failure.
// ---------------------------------------------------------------------------

#[test]
fn mads_bootstrap_list_oob_diff_returns_err_not_panic() {
    let initial = include_bytes!("fixtures_march_2026/mads_bootstrap__initial.bin");
    let op1 = include_bytes!("fixtures_march_2026/mads_bootstrap__op1.bin");

    let doc = LoroDoc::new();
    doc.import(initial).expect("initial snapshot imports");
    let err = doc
        .import(op1)
        .expect_err("malformed list diff should surface as Err");
    let msg = format!("{err}");
    assert!(
        msg.contains("list diff"),
        "expected a list-diff error, got: {msg}",
    );
}

// ---------------------------------------------------------------------------
// Bug #3: OnceCell double-set in loro_dag::get_vv on diamond deps during
// `importBatch` (GH loro-dev/loro#929).
// ---------------------------------------------------------------------------

#[test]
fn pr929_import_batch_diamond_does_not_panic() {
    let initial = include_bytes!("fixtures_march_2026/pr929_import_batch__initial.bin");
    let ops: &[&[u8]] = &[
        include_bytes!("fixtures_march_2026/pr929_import_batch__op1.bin"),
        include_bytes!("fixtures_march_2026/pr929_import_batch__op2.bin"),
        include_bytes!("fixtures_march_2026/pr929_import_batch__op3.bin"),
        include_bytes!("fixtures_march_2026/pr929_import_batch__op4.bin"),
        include_bytes!("fixtures_march_2026/pr929_import_batch__op5.bin"),
        include_bytes!("fixtures_march_2026/pr929_import_batch__op6.bin"),
        include_bytes!("fixtures_march_2026/pr929_import_batch__op7.bin"),
        include_bytes!("fixtures_march_2026/pr929_import_batch__op8.bin"),
        include_bytes!("fixtures_march_2026/pr929_import_batch__op9.bin"),
    ];

    let mut blobs: Vec<Vec<u8>> = Vec::with_capacity(ops.len() + 1);
    blobs.push(initial.to_vec());
    for op in ops {
        blobs.push(op.to_vec());
    }

    let doc = LoroDoc::new();
    doc.import_batch(&blobs)
        .expect("diamond-dep batch imports without panic");
}
