//! Merge semantics of a shallow-snapshot-bootstrapped doc when it meets
//! concurrent peers that still hold full history.
//!
//! These tests pin down the behavior a sync layer relies on when it uploads
//! `shallow-snapshot` blobs instead of full snapshots. The design note
//! `docs/shallow-snapshot-concurrency.md` summarizes the guarantees and
//! references these tests by name.
//!
//! Scenario vocabulary used throughout:
//! - `A`: full-history doc. `V` and `F` are versions of A with `V < F`; F is
//!   the shallow root.
//! - `B`: doc bootstrapped by importing A's shallow snapshot at F.
//! - `C`: a peer holding full history up to some version, making updates on
//!   top of it.

use loro::{ExportMode, Frontiers, IdSpan, LoroDoc, LoroError, TreeParentId, VersionVector};

/// Build the shared fixture: doc A (peer 1) with map/list/text/movable-list/
/// tree content, edited in three phases. Returns the doc plus the version
/// vectors and frontiers at V (after phase 1), F (after phase 2, the shallow
/// root), and the tip (after phase 3).
fn build_doc_a() -> (LoroDoc, VersionVector, Frontiers, VersionVector, Frontiers) {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();

    // Phase 1: content that ends up strictly before the shallow root.
    a.get_map("map").insert("a", 1).unwrap();
    a.get_list("list").insert(0, "l0").unwrap();
    a.get_text("text").insert(0, "hello").unwrap();
    let movable = a.get_movable_list("movable");
    movable.insert(0, "m0").unwrap();
    movable.insert(1, "m1").unwrap();
    let tree = a.get_tree("tree");
    tree.enable_fractional_index(0);
    let root = tree.create(TreeParentId::Root).unwrap();
    a.commit();
    let v_vv = a.oplog_vv();
    let v_frontiers = a.oplog_frontiers();

    // Phase 2: content between V and F; F becomes the shallow root.
    a.get_map("map").insert("b", 2).unwrap();
    a.get_list("list").insert(1, "l1").unwrap();
    a.get_text("text").insert(5, " world").unwrap();
    movable.mov(0, 1).unwrap();
    let child = tree.create(root).unwrap();
    a.commit();
    let f_vv = a.oplog_vv();
    let f_frontiers = a.oplog_frontiers();

    // Phase 3: retained history after the shallow root.
    a.get_map("map").insert("c", 3).unwrap();
    a.get_list("list").insert(2, "l2").unwrap();
    a.get_text("text").insert(11, "!").unwrap();
    movable.insert(2, "m2").unwrap();
    tree.create(child).unwrap();
    a.commit();

    (a, v_vv, v_frontiers, f_vv, f_frontiers)
}

/// Bootstrap B from A's shallow snapshot at F and assert the root metadata.
fn bootstrap_b(a: &LoroDoc, f_vv: &VersionVector, f_frontiers: &Frontiers) -> LoroDoc {
    let blob = a.export(ExportMode::shallow_snapshot(f_frontiers)).unwrap();
    let b = LoroDoc::new();
    b.import(&blob).unwrap();
    assert!(b.is_shallow());
    assert_eq!(b.shallow_since_frontiers(), *f_frontiers);
    // The ops included by shallow_since_vv are NOT in the doc; the shallow
    // root frontier op itself is retained, so the vv ends at the frontier
    // counter (exclusive of it) rather than at the vv of F.
    let mut expected = f_vv.clone();
    for id in f_frontiers.iter() {
        expected.insert(id.peer, id.counter);
    }
    for (peer, counter) in expected.iter() {
        assert_eq!(
            b.shallow_since_vv().get(peer).copied(),
            Some(*counter),
            "shallow_since_vv must exclude exactly the ops before the shallow root"
        );
    }
    // The shallow replica shows the same latest state as the full doc.
    assert_eq!(b.get_deep_value(), a.get_deep_value());
    b
}

/// Case 1b/1c companions: history already included in the shallow root is a
/// no-op, and the shallow root metadata does not move.
#[test]
fn shallow_bootstrap_and_rereading_old_history_is_noop() -> anyhow::Result<()> {
    let (a, _v_vv, v_frontiers, f_vv, f_frontiers) = build_doc_a();
    let b = bootstrap_b(&a, &f_vv, &f_frontiers);
    let b_value = b.get_deep_value();

    // Case 1c: re-importing updates whose causal past is entirely before F
    // (already included in the shallow root state) is a no-op.
    let old_history = a.export(ExportMode::snapshot_at(&v_frontiers))?;
    let status = b.import(&old_history)?;
    assert!(status.pending.is_none());
    assert_eq!(b.get_deep_value(), b_value);
    assert!(b.is_shallow());
    assert_eq!(b.shallow_since_frontiers(), f_frontiers);

    // Re-importing A's full snapshot is also a no-op for B.
    let status = b.import(&a.export(ExportMode::Snapshot)?)?;
    assert!(status.pending.is_none());
    assert_eq!(b.get_deep_value(), b_value);
    assert_eq!(b.shallow_since_frontiers(), f_frontiers);

    Ok(())
}

/// Case 1a: updates whose causal past is before the shallow root are rejected
/// with `ImportUpdatesThatDependsOnOutdatedVersion`; they are neither applied
/// nor parked as pending.
#[test]
fn update_based_on_version_before_shallow_root_is_rejected() -> anyhow::Result<()> {
    let (a, v_vv, v_frontiers, f_vv, f_frontiers) = build_doc_a();
    let b = bootstrap_b(&a, &f_vv, &f_frontiers);
    let b_value = b.get_deep_value();

    // C holds full history up to V < F and edits on top of it.
    let c = LoroDoc::new();
    c.import(&a.export(ExportMode::snapshot_at(&v_frontiers))?)?;
    c.set_peer_id(3)?;
    c.get_map("map").insert("from_c", true)?;
    c.commit();
    let c_updates = c.export(ExportMode::updates(&v_vv))?;

    let err = b.import(&c_updates).unwrap_err();
    assert!(
        matches!(err, LoroError::ImportUpdatesThatDependsOnOutdatedVersion),
        "expected ImportUpdatesThatDependsOnOutdatedVersion, got {err:?}"
    );
    // Nothing applied, nothing pending, root metadata untouched.
    assert_eq!(b.get_deep_value(), b_value);
    assert!(b.is_shallow());
    assert_eq!(b.shallow_since_frontiers(), f_frontiers);
    assert!(b.get_map("map").get("from_c").is_none());

    // A peer that never synced with A at all is rejected the same way: its
    // genesis change has empty deps, which the shallow doc treats as rooted
    // before the shallow root.
    let d = LoroDoc::new();
    d.set_peer_id(9)?;
    d.get_text("text").insert(0, "stranger")?;
    d.commit();
    let err = b.import(&d.export(ExportMode::all_updates())?).unwrap_err();
    assert!(matches!(
        err,
        LoroError::ImportUpdatesThatDependsOnOutdatedVersion
    ));

    Ok(())
}

/// Case 4: updates with missing dependencies that are NOT before the shallow
/// root are parked as pending and applied once the dependency arrives; the
/// import status reports both transitions.
#[test]
fn pending_updates_after_root_apply_when_dependency_arrives() -> anyhow::Result<()> {
    let (a, _v_vv, _v_frontiers, f_vv, f_frontiers) = build_doc_a();
    let b = bootstrap_b(&a, &f_vv, &f_frontiers);
    let b_value = b.get_deep_value();

    // C syncs with A exactly at F, then commits two changes in sequence.
    let c = LoroDoc::new();
    c.import(&a.export(ExportMode::snapshot_at(&f_frontiers))?)?;
    c.set_peer_id(11)?;
    c.get_map("map").insert("step1", 1)?;
    c.commit();
    let end1 = *c.oplog_vv().get(&11).unwrap();
    c.get_map("map").insert("step2", 2)?;
    c.commit();
    let end2 = *c.oplog_vv().get(&11).unwrap();
    let first = c.export(ExportMode::updates_in_range(vec![IdSpan::new(11, 0, end1)]))?;
    let second = c.export(ExportMode::updates_in_range(vec![IdSpan::new(
        11, end1, end2,
    )]))?;

    // Importing the second change first: pending, not applied, no error.
    let status = b.import(&second)?;
    let pending = status.pending.expect("second change should be pending");
    assert_eq!(pending.get(&11), Some(&(end1, end2)));
    assert_eq!(b.get_deep_value(), b_value);

    // Once the missing dependency arrives, both changes apply.
    let status = b.import(&first)?;
    assert!(status.pending.is_none());
    assert_eq!(status.success.get(&11), Some(&(0, end2)));
    assert_eq!(
        b.get_map("map").get("step1").map(|v| v.get_deep_value()),
        Some(1.into())
    );
    assert_eq!(
        b.get_map("map").get("step2").map(|v| v.get_deep_value()),
        Some(2.into())
    );

    Ok(())
}

/// A concurrent peer's updates that depend on its own pre-root history become
/// pending, but can never be applied: the chain's genesis change is rejected
/// as rooted before the shallow root. This is the one lossy case; see
/// docs/shallow-snapshot-concurrency.md.
#[test]
fn concurrent_chain_rooted_before_shallow_root_can_never_merge() -> anyhow::Result<()> {
    let (a, _v_vv, _v_frontiers, f_vv, f_frontiers) = build_doc_a();
    let b = bootstrap_b(&a, &f_vv, &f_frontiers);
    let b_value = b.get_deep_value();

    // D never synced with A: d1 is a genesis change, d2 depends on d1. Both
    // are concurrent with F.
    let d = LoroDoc::new();
    d.set_peer_id(13)?;
    d.get_map("map").insert("d1", 1)?;
    d.commit();
    let end1 = *d.oplog_vv().get(&13).unwrap();
    d.get_map("map").insert("d2", 2)?;
    d.commit();
    let end2 = *d.oplog_vv().get(&13).unwrap();
    let d1 = d.export(ExportMode::updates_in_range(vec![IdSpan::new(13, 0, end1)]))?;
    let d2 = d.export(ExportMode::updates_in_range(vec![IdSpan::new(
        13, end1, end2,
    )]))?;

    // d2's deps are unknown to B but not before the shallow root, so d2 is
    // parked as pending rather than rejected.
    let status = b.import(&d2)?;
    assert!(status.pending.is_some());
    assert_eq!(b.get_deep_value(), b_value);

    // d1 is a genesis change; B rejects it as rooted before the shallow root,
    // so the parked d2 can never be unlocked.
    let err = b.import(&d1).unwrap_err();
    assert!(matches!(
        err,
        LoroError::ImportUpdatesThatDependsOnOutdatedVersion
    ));
    assert_eq!(b.get_deep_value(), b_value);
    assert!(b.get_map("map").get("d1").is_none());
    assert!(b.get_map("map").get("d2").is_none());

    Ok(())
}

/// Case 1b: updates causally after the shallow root apply normally.
#[test]
fn updates_causally_after_shallow_root_apply() -> anyhow::Result<()> {
    let (a, _v_vv, _v_frontiers, f_vv, f_frontiers) = build_doc_a();
    let b = bootstrap_b(&a, &f_vv, &f_frontiers);

    // C holds full history up to F and edits on top of it.
    let c = LoroDoc::new();
    c.import(&a.export(ExportMode::snapshot_at(&f_frontiers))?)?;
    c.set_peer_id(5)?;
    c.get_map("map").insert("after_f", "yes")?;
    c.get_text("text").insert(0, "C:")?;
    c.commit();
    let c_updates = c.export(ExportMode::updates(&f_vv))?;

    let status = b.import(&c_updates)?;
    assert!(status.pending.is_none());
    // The shallow doc converges with a full-history doc that saw the same
    // updates.
    let full = LoroDoc::new();
    full.import(&a.export(ExportMode::Snapshot)?)?;
    full.import(&c_updates)?;
    assert_eq!(b.get_deep_value(), full.get_deep_value());
    // Importing post-root updates does not move the shallow root.
    assert!(b.is_shallow());
    assert_eq!(b.shallow_since_frontiers(), f_frontiers);

    Ok(())
}

/// Case 2: the reverse direction. Updates exported from the shallow doc apply
/// to a full-history doc, and importing the shallow doc's shallow snapshot
/// into a full-history doc transfers the retained history without making the
/// target shallow.
#[test]
fn shallow_doc_merges_back_into_full_history_doc() -> anyhow::Result<()> {
    let (a, _v_vv, _v_frontiers, f_vv, f_frontiers) = build_doc_a();
    let b = bootstrap_b(&a, &f_vv, &f_frontiers);

    // B edits on top of its shallow state.
    b.set_peer_id(7)?;
    b.get_map("map").insert("from_b", "b")?;
    b.get_list("list").insert(3, "l3")?;
    b.commit();

    // 2a: A' has full history and never saw the shallow snapshot. B's updates
    // (causally after F) apply normally.
    let a_prime = LoroDoc::new();
    a_prime.import(&a.export(ExportMode::Snapshot)?)?;
    let b_updates = b.export(ExportMode::updates(&a_prime.oplog_vv()))?;
    let status = a_prime.import(&b_updates)?;
    assert!(status.pending.is_none());
    assert_eq!(a_prime.get_deep_value(), b.get_deep_value());
    assert!(!a_prime.is_shallow());

    // 2b: A'' has full history (more than the shallow root) and imports B's
    // shallow snapshot (re-exported at the same root F) directly. The snapshot
    // is routed through its retained changes, so A'' receives B's edits but
    // keeps its full history.
    let a_second = LoroDoc::new();
    a_second.import(&a.export(ExportMode::Snapshot)?)?;
    let b_shallow = b.export(ExportMode::shallow_snapshot(&f_frontiers))?;
    let status = a_second.import(&b_shallow)?;
    assert!(status.pending.is_none());
    assert_eq!(a_second.get_deep_value(), b.get_deep_value());
    assert!(!a_second.is_shallow());
    assert!(a_second.shallow_since_vv().iter().next().is_none());
    // Full history is still available: a checkout to V works on A'' while it
    // is rejected on B.
    assert!(a_second.checkout(&Frontiers::default()).is_ok());

    Ok(())
}
