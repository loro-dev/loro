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

fn doc_with_independent_text_heads(peer_order: &[u64]) -> LoroDoc {
    let doc = LoroDoc::new();
    let Some((&first_peer, remaining_peers)) = peer_order.split_first() else {
        return doc;
    };
    doc.set_peer_id(first_peer).unwrap();
    let first_value = char::from_u32(0x40 + first_peer as u32)
        .unwrap()
        .to_string();
    doc.get_text("text").insert(0, &first_value).unwrap();
    doc.commit();
    for &peer in remaining_peers {
        let other = LoroDoc::new();
        other.set_peer_id(peer).unwrap();
        let value = char::from_u32(0x40 + peer as u32).unwrap().to_string();
        other.get_text("text").insert(0, &value).unwrap();
        other.commit();
        doc.import(&other.export(ExportMode::all_updates()).unwrap())
            .unwrap();
    }
    doc
}

fn doc_with_shared_root(peer_order: &[u64]) -> (LoroDoc, Frontiers, Frontiers) {
    let base = LoroDoc::new();
    base.set_peer_id(100).unwrap();
    base.get_text("text").insert(0, "before").unwrap();
    base.commit();
    let before_root = base.oplog_frontiers();
    base.get_text("text").insert(6, " root").unwrap();
    base.commit();
    let root = base.oplog_frontiers();
    let base_snapshot = base.export(ExportMode::Snapshot).unwrap();

    let aggregate = LoroDoc::from_snapshot(&base_snapshot).unwrap();
    for &peer in peer_order {
        let branch = LoroDoc::from_snapshot(&base_snapshot).unwrap();
        branch.set_peer_id(peer).unwrap();
        let text = branch.get_text("text");
        text.insert(text.len_unicode(), &format!(" [{peer}]"))
            .unwrap();
        branch.commit();
        aggregate
            .import(&branch.export(ExportMode::all_updates()).unwrap())
            .unwrap();
    }

    (aggregate, root, before_root)
}

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

/// The change immediately concurrent with F must be rejected too: its deps
/// equal F's own deps, which have already been trimmed from the shallow DAG.
#[test]
fn update_concurrent_with_root_frontier_is_rejected() -> anyhow::Result<()> {
    let a = LoroDoc::new();
    a.set_peer_id(1)?;
    a.get_map("m").insert("before", 1)?;
    a.commit();
    let v = a.oplog_frontiers();
    let vv = a.oplog_vv();
    a.get_map("m").insert("root", 2)?;
    a.commit();
    let f = a.oplog_frontiers();
    let b = LoroDoc::from_snapshot(&a.export(ExportMode::shallow_snapshot(&f))?)?;
    let before = b.get_deep_value();
    let c = LoroDoc::from_snapshot(&a.export(ExportMode::snapshot_at(&v))?)?;
    c.set_peer_id(2)?;
    c.get_map("m").insert("concurrent", true)?;
    c.commit();
    let err = b.import(&c.export(ExportMode::updates(&vv))?).unwrap_err();
    assert!(matches!(
        err,
        LoroError::ImportUpdatesThatDependsOnOutdatedVersion
    ));
    assert_eq!(b.get_deep_value(), before);
    assert_eq!(b.shallow_since_frontiers(), f);
    // Rejection leaves the doc usable for valid post-root updates.
    a.get_map("m").insert("after", 3)?;
    a.commit();
    let status = b.import(&a.export(ExportMode::updates(&b.oplog_vv()))?)?;
    assert!(status.pending.is_none());
    assert_eq!(b.get_deep_value(), a.get_deep_value());
    Ok(())
}

#[test]
fn shallow_snapshot_with_multiple_heads_imports_into_empty_doc() -> anyhow::Result<()> {
    for peers in 1..=12 {
        let peer_order: Vec<u64> = (1..=peers).collect();
        let source = doc_with_independent_text_heads(&peer_order);
        let frontiers = source.oplog_frontiers();
        assert_eq!(frontiers.len(), peers as usize);
        let expected = source.get_text("text").to_string();
        let shallow = source.export(ExportMode::shallow_snapshot(&frontiers))?;
        let state_only = source.export(ExportMode::state_only(Some(&frontiers)))?;

        let meta = LoroDoc::decode_import_blob_meta(&shallow, false)?;
        if peers == 1 {
            assert_eq!(meta.start_frontiers, frontiers);
        } else {
            assert!(meta.start_frontiers.is_empty());
        }
        assert_eq!(meta.partial_end_vv, source.oplog_vv());

        let nonempty = LoroDoc::new();
        nonempty.set_peer_id(100)?;
        nonempty.get_text("other").insert(0, "z")?;
        nonempty.commit();
        nonempty.import(&shallow)?;
        assert_eq!(nonempty.get_text("text").to_string(), expected);

        let state_only_target = LoroDoc::new();
        state_only_target.import(&state_only)?;
        assert_eq!(state_only_target.get_text("text").to_string(), expected);

        let empty = LoroDoc::new();
        let result = empty.import(&shallow);
        assert!(
            result.is_ok(),
            "{peers} independent heads should import into an empty doc, got {result:?}"
        );
        assert_eq!(empty.get_text("text").to_string(), expected);
        assert_eq!(empty.oplog_frontiers(), source.oplog_frontiers());
        assert_eq!(empty.state_frontiers(), source.state_frontiers());
        assert_eq!(empty.oplog_vv(), source.oplog_vv());
    }

    Ok(())
}

#[test]
fn shallow_snapshot_multi_head_root_is_order_independent_and_syncable() -> anyhow::Result<()> {
    let (source, root, before_root) = doc_with_shared_root(&[1, 2, 3, 4, 5]);
    let (reverse, reverse_root, _) = doc_with_shared_root(&[5, 4, 3, 2, 1]);
    assert_eq!(source.oplog_frontiers().len(), 5);
    assert_eq!(source.oplog_frontiers(), reverse.oplog_frontiers());
    assert_eq!(source.get_deep_value(), reverse.get_deep_value());
    assert_eq!(root, reverse_root);

    let shallow = source.export(ExportMode::shallow_snapshot(&source.oplog_frontiers()))?;
    let reverse_shallow =
        reverse.export(ExportMode::shallow_snapshot(&reverse.oplog_frontiers()))?;
    let meta = LoroDoc::decode_import_blob_meta(&shallow, false)?;
    let reverse_meta = LoroDoc::decode_import_blob_meta(&reverse_shallow, false)?;
    assert_eq!(meta.start_frontiers, root);
    assert_eq!(reverse_meta.start_frontiers, root);
    assert_eq!(meta.partial_end_vv, source.oplog_vv());
    assert_eq!(reverse_meta.partial_end_vv, source.oplog_vv());

    let imported = LoroDoc::from_snapshot(&shallow)?;
    assert!(imported.is_shallow());
    assert_eq!(imported.shallow_since_frontiers(), root);
    assert_eq!(imported.oplog_frontiers(), source.oplog_frontiers());
    assert_eq!(imported.oplog_vv(), source.oplog_vv());
    assert_eq!(imported.get_deep_value(), source.get_deep_value());
    assert_eq!(
        imported.checkout(&before_root).unwrap_err(),
        LoroError::SwitchToVersionBeforeShallowRoot
    );

    imported.set_peer_id(200)?;
    let imported_text = imported.get_text("text");
    imported_text.insert(imported_text.len_unicode(), " shallow")?;
    imported.commit();
    source.import(&imported.export(ExportMode::updates(&source.oplog_vv()))?)?;

    source.set_peer_id(201)?;
    let source_text = source.get_text("text");
    source_text.insert(source_text.len_unicode(), " full")?;
    source.commit();
    imported.import(&source.export(ExportMode::updates(&imported.oplog_vv()))?)?;
    assert_eq!(imported.get_deep_value(), source.get_deep_value());
    assert_eq!(imported.oplog_frontiers(), source.oplog_frontiers());
    assert_eq!(imported.oplog_vv(), source.oplog_vv());
    assert_eq!(imported.shallow_since_frontiers(), root);

    Ok(())
}

#[test]
fn shallow_snapshot_handles_partially_shared_and_merged_heads() -> anyhow::Result<()> {
    let base = LoroDoc::new();
    base.set_peer_id(300)?;
    base.get_text("text").insert(0, "before")?;
    base.commit();
    base.get_text("text").insert(6, " root")?;
    base.commit();
    let root = base.oplog_frontiers();
    let base_snapshot = base.export(ExportMode::Snapshot)?;

    let shared = LoroDoc::from_snapshot(&base_snapshot)?;
    shared.set_peer_id(301)?;
    shared.get_text("text").insert(11, " shared")?;
    shared.commit();
    let shared_snapshot = shared.export(ExportMode::Snapshot)?;

    let left = LoroDoc::from_snapshot(&shared_snapshot)?;
    left.set_peer_id(302)?;
    let left_text = left.get_text("text");
    left_text.insert(left_text.len_unicode(), " left")?;
    left.commit();
    let right = LoroDoc::from_snapshot(&shared_snapshot)?;
    right.set_peer_id(303)?;
    let right_text = right.get_text("text");
    right_text.insert(right_text.len_unicode(), " right")?;
    right.commit();

    let outside = LoroDoc::from_snapshot(&base_snapshot)?;
    outside.set_peer_id(304)?;
    outside.get_text("text").insert(11, " outside")?;
    outside.commit();

    let partial = LoroDoc::from_snapshot(&base_snapshot)?;
    partial.import(&left.export(ExportMode::all_updates())?)?;
    partial.import(&right.export(ExportMode::all_updates())?)?;
    partial.import(&outside.export(ExportMode::all_updates())?)?;
    assert_eq!(partial.oplog_frontiers().len(), 3);
    let partial_blob = partial.export(ExportMode::shallow_snapshot(&partial.oplog_frontiers()))?;
    let partial_meta = LoroDoc::decode_import_blob_meta(&partial_blob, false)?;
    assert_eq!(partial_meta.start_frontiers, root);
    let partial_import = LoroDoc::from_snapshot(&partial_blob)?;
    assert_eq!(partial_import.shallow_since_frontiers(), root);
    assert_eq!(partial_import.get_deep_value(), partial.get_deep_value());
    assert_eq!(partial_import.oplog_vv(), partial.oplog_vv());

    let merged = LoroDoc::from_snapshot(&base_snapshot)?;
    merged.import(&left.export(ExportMode::all_updates())?)?;
    merged.import(&right.export(ExportMode::all_updates())?)?;
    merged.set_peer_id(305)?;
    let merged_text = merged.get_text("text");
    merged_text.insert(merged_text.len_unicode(), " merged")?;
    merged.commit();

    let merged_with_outside = LoroDoc::from_snapshot(&base_snapshot)?;
    merged_with_outside.import(&merged.export(ExportMode::all_updates())?)?;
    merged_with_outside.import(&outside.export(ExportMode::all_updates())?)?;
    assert_eq!(merged_with_outside.oplog_frontiers().len(), 2);
    let merged_blob = merged_with_outside.export(ExportMode::shallow_snapshot(
        &merged_with_outside.oplog_frontiers(),
    ))?;
    let merged_meta = LoroDoc::decode_import_blob_meta(&merged_blob, false)?;
    assert_eq!(merged_meta.start_frontiers, root);
    let merged_import = LoroDoc::from_snapshot(&merged_blob)?;
    assert_eq!(merged_import.shallow_since_frontiers(), root);
    assert_eq!(
        merged_import.get_deep_value(),
        merged_with_outside.get_deep_value()
    );
    assert_eq!(merged_import.oplog_vv(), merged_with_outside.oplog_vv());

    Ok(())
}
