#![allow(deprecated)]
#![allow(unexpected_cfgs)]
use loro::{
    cursor::Cursor, ContainerID, ContainerTrait, EncodedBlobMode, ExportMode, LoroDoc, LoroError,
    LoroList, LoroText, UndoManager,
};
use std::sync::{Arc, Mutex};
use tracing::{trace, trace_span};

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

#[test]
fn test_event_hint_cross_container_merge_bug() {
    let doc = LoroDoc::new();
    let text_a = doc.get_text("text_a");
    let text_b = doc.get_text("text_b");

    // Insert initial content
    text_a.insert(0, "a").unwrap();
    text_b.insert(0, "b").unwrap();
    doc.commit();

    // Track events
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();

    let _guard = doc.subscribe_root(Arc::new(move |batch| {
        for event in batch.events {
            events_clone
                .lock()
                .unwrap()
                .push(event.target.name().to_string());
        }
    }));

    // Delete from both containers - this should generate 2 events
    text_a.delete(0, 1).unwrap();
    text_b.delete(0, 1).unwrap();
    doc.commit();

    // Bug: Only 1 event is generated instead of 2
    let events = events.lock().unwrap();
    assert_eq!(
        events.len(),
        2,
        "Expected 2 events, got {}: {:?}",
        events.len(),
        *events
    );
}

#[test]
fn test_event_hint_bug_reproduction() {
    // This test specifically reproduces the EventHint merge bug
    // by creating delete operations that will be merged incorrectly
    let doc = LoroDoc::new();
    let text_a = doc.get_text("text_a");
    let text_b = doc.get_text("text_b");

    // Insert content
    text_a.insert(0, "hello").unwrap();
    text_b.insert(0, "world").unwrap();
    doc.commit();

    // Track detailed event information
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();

    let _guard = doc.subscribe_root(Arc::new(move |event_batch| {
        let mut events_lock = events_clone.lock().unwrap();

        for event in event_batch.events.iter() {
            let container_name = event.target.name().as_str().to_string();

            if let Some(text_diff) = event.diff.as_text() {
                // Count total operations in the diff
                let mut total_ops = 0;
                let mut delete_ops = 0;
                let mut retain_ops = 0;

                for delta in text_diff.iter() {
                    total_ops += 1;
                    let delta_str = format!("{delta:?}");
                    if delta_str.contains("Delete") {
                        delete_ops += 1;
                    } else if delta_str.contains("Retain") {
                        retain_ops += 1;
                    }
                }

                events_lock.push((container_name, total_ops, delete_ops, retain_ops));
            }
        }
    }));

    // Perform operations that should trigger the bug
    // Delete from position 0 in text_a (deletes 'h')
    text_a.delete(0, 1).unwrap();
    // Delete from position 0 in text_b (deletes 'w')
    text_b.delete(0, 1).unwrap();
    doc.commit();

    let events_lock = events.lock().unwrap();

    println!("\n=== Bug Reproduction Test ===");
    println!("Events received: {:?}", *events_lock);

    // The bug would cause these events to be merged incorrectly
    // We should have 2 events, one for each container
    assert_eq!(
        events_lock.len(),
        2,
        "Should have exactly 2 events, got {}",
        events_lock.len()
    );

    // Each event should only contain operations for its own container
    let text_a_events: Vec<_> = events_lock
        .iter()
        .filter(|(name, _, _, _)| name == "text_a")
        .collect();
    let text_b_events: Vec<_> = events_lock
        .iter()
        .filter(|(name, _, _, _)| name == "text_b")
        .collect();

    assert_eq!(text_a_events.len(), 1, "text_a should have exactly 1 event");
    assert_eq!(text_b_events.len(), 1, "text_b should have exactly 1 event");

    // Check the operations count
    if let Some((_, total_ops, delete_ops, _)) = text_a_events.first() {
        assert_eq!(*total_ops, 1, "text_a should have 1 operation");
        assert_eq!(*delete_ops, 1, "text_a should have 1 delete operation");
    }

    if let Some((_, total_ops, delete_ops, retain_ops)) = text_b_events.first() {
        // text_b might have a retain operation if the bug manifests
        println!(
            "text_b operations - total: {total_ops}, deletes: {delete_ops}, retains: {retain_ops}"
        );
        // If the bug exists, text_b might show unexpected operations
    }

    // Verify final state
    assert_eq!(text_a.to_string(), "ello");
    assert_eq!(text_b.to_string(), "orld");
}

#[test]
fn test_event_hint_merge_bug_clear_demonstration() {
    // This test clearly demonstrates the EventHint merge bug
    let doc = LoroDoc::new();
    let text_a = doc.get_text("text_a");
    let text_b = doc.get_text("text_b");

    // Insert content
    text_a.insert(0, "12345").unwrap();
    text_b.insert(0, "abcde").unwrap();
    doc.commit();

    // Track which containers received events
    let event_containers = Arc::new(Mutex::new(Vec::new()));
    let event_containers_clone = event_containers.clone();

    let _guard = doc.subscribe_root(Arc::new(move |event_batch| {
        let mut containers = event_containers_clone.lock().unwrap();

        println!("\n=== Event Batch ===");
        println!("Total events in batch: {}", event_batch.events.len());

        for (idx, event) in event_batch.events.iter().enumerate() {
            let container_name = event.target.name().as_str().to_string();
            println!("Event #{idx}: Container '{container_name}'");

            if let Some(text_diff) = event.diff.as_text() {
                println!("  Diff operations:");
                for (i, delta) in text_diff.iter().enumerate() {
                    println!("    Operation #{i}: {delta:?}");
                }
            }

            containers.push(container_name);
        }
        println!("=== End Batch ===\n");
    }));

    println!("\nPerforming delete operations:");
    println!("- Deleting position 0 from text_a (removes '1')");
    println!("- Deleting position 0 from text_b (removes 'a')");

    // These two operations should generate two separate events
    // But due to the bug, they might be merged into one
    text_a.delete(0, 1).unwrap();
    text_b.delete(0, 1).unwrap();
    doc.commit();

    let containers = event_containers.lock().unwrap();

    // This assertion will fail if the bug is present
    assert_eq!(
        containers.len(),
        2,
        "Expected 2 events (one for each container), but got {}. Events: {:?}",
        containers.len(),
        *containers
    );

    // Check that both containers received their own events
    let text_a_count = containers.iter().filter(|&c| c == "text_a").count();
    let text_b_count = containers.iter().filter(|&c| c == "text_b").count();

    assert_eq!(text_a_count, 1, "text_a should have exactly 1 event");
    assert_eq!(text_b_count, 1, "text_b should have exactly 1 event");

    // Verify the final state is correct
    assert_eq!(text_a.to_string(), "2345");
    assert_eq!(text_b.to_string(), "bcde");
}

#[test]
fn test_undo_counter_after_remote_update_issue_905() {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1).unwrap();
    let mut undo_manager = UndoManager::new(&doc_a);
    undo_manager.set_merge_interval(0);

    let counter_a = doc_a.get_counter("counter");
    counter_a.increment(1.0).unwrap();
    doc_a.commit();

    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2).unwrap();
    doc_b
        .import(&doc_a.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let counter_b = doc_b.get_counter("counter");
    assert_eq!(counter_b.get_value(), 1.0);
    counter_b.increment(1.0).unwrap();
    doc_b.commit();

    doc_a
        .import(&doc_b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(counter_a.get_value(), 2.0);

    assert!(undo_manager.can_undo());
    assert!(undo_manager.undo().unwrap());
    assert_eq!(counter_a.get_value(), 1.0);

    assert!(undo_manager.can_redo());
    assert!(undo_manager.redo().unwrap());
    assert_eq!(counter_a.get_value(), 2.0);
}

#[test]
fn undo_max_steps_trim_after_remote_undo_should_not_panic() -> Result<(), LoroError> {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1)?;
    let text_a = doc_a.get_text("text");
    let mut undo_manager = UndoManager::new(&doc_a);
    undo_manager.set_merge_interval(0);
    undo_manager.set_max_undo_steps(3);

    text_a.insert(0, "A")?;
    doc_a.commit();

    let doc_b = LoroDoc::from_snapshot(&doc_a.export(ExportMode::Snapshot)?)?;
    doc_b.set_peer_id(2)?;
    let text_b = doc_b.get_text("text");
    text_b.insert(0, "R")?;
    doc_b.commit();

    doc_a.import(&doc_b.export(ExportMode::all_updates())?)?;
    assert!(undo_manager.undo()?);
    assert_eq!(text_a.to_string(), "R");

    for i in 0..4 {
        text_a.insert(text_a.len_unicode(), &i.to_string())?;
        doc_a.commit();
    }

    assert_eq!(undo_manager.undo_count(), 3);
    Ok(())
}

#[test]
fn import_twice() {
    let doc = LoroDoc::new();
    let base64 = "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";
    let decoded_bytes = base64::decode(base64).expect("base64 decode error");
    doc.import(&decoded_bytes).unwrap();
    doc.import(&decoded_bytes).unwrap();
}

#[test]
fn import_doc_err() {
    let base64 = include_bytes!("./issue_import.base64.txt");
    let base64 = str::from_utf8(base64).unwrap();
    let decoded_bytes = base64::decode(base64).expect("base64 decode error");

    let doc = LoroDoc::new();
    doc.import(&decoded_bytes).unwrap();
    dbg!(doc.get_deep_value());
}

#[test]
fn undo_tree_mov_between_children() {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    let tree = doc.get_tree("tree");
    let a = tree.create(None).unwrap();
    tree.get_meta(a).unwrap().insert("title", "A").unwrap();
    doc.commit();
    let b = tree.create(None).unwrap();
    tree.get_meta(b).unwrap().insert("title", "B").unwrap();
    doc.commit();
    let doc_value_0 = doc.get_deep_value();
    tree.mov_after(a, b).unwrap();
    undo.undo().unwrap();
    let doc_value_1 = doc.get_deep_value();
    assert_eq!(doc_value_0, doc_value_1);
}

#[test]
fn issue_822_tree_shallow_snapshot_roundtrip() {
    let snapshot_bytes = include_bytes!("./issue_822.bin");
    let doc = LoroDoc::new();
    doc.import(snapshot_bytes).expect("import snapshot blob");

    let tree = doc.get_tree("nodes");
    let tree_before = tree.get_value();
    let doc_before = doc.get_value();

    let snapshot_meta =
        LoroDoc::decode_import_blob_meta(snapshot_bytes, false).expect("decode snapshot meta");
    assert!(snapshot_meta.mode.is_snapshot());
    let imported_is_shallow = snapshot_meta.mode == EncodedBlobMode::ShallowSnapshot;

    let frontiers = doc.state_frontiers();
    let shallow_bytes = trace_span!("EXPORT").in_scope(|| {
        doc.export(ExportMode::shallow_snapshot(&frontiers))
            .expect("export shallow snapshot")
    });

    let snapshot_meta_1 = LoroDoc::decode_import_blob_meta(&shallow_bytes, false).unwrap();
    assert!(matches!(
        snapshot_meta_1.mode,
        EncodedBlobMode::ShallowSnapshot
    ));

    let shallow_meta =
        LoroDoc::decode_import_blob_meta(&shallow_bytes, false).expect("decode shallow meta");
    assert_eq!(shallow_meta.mode, EncodedBlobMode::ShallowSnapshot);

    let shallow_doc = LoroDoc::new();
    trace_span!("FINAL_IMPORT").in_scope(|| {
        trace!("bytes.len: {}", shallow_bytes.len());
        shallow_doc
            .import(&shallow_bytes)
            .expect("import shallow snapshot");
    });

    assert!(shallow_doc.is_shallow());
    assert_eq!(doc.is_shallow(), imported_is_shallow);

    let tree_after = shallow_doc.get_tree("nodes").get_value();
    let doc_after = shallow_doc.get_value();

    assert_eq!(
        tree_before, tree_after,
        "tree shallow value should roundtrip"
    );
    assert_eq!(doc_before, doc_after, "doc shallow value should roundtrip");
}

#[test]
fn issue_928_diff_before_shallow_root_should_error_without_poisoning_doc() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();

    let text = doc.get_text("t");
    text.insert(0, "hello").unwrap();
    doc.commit();
    let pre_shallow_frontiers = doc.oplog_frontiers();

    text.insert(5, " world").unwrap();
    doc.commit();
    let current_frontiers = doc.oplog_frontiers();

    let shallow_snapshot = doc
        .export(ExportMode::shallow_snapshot(&current_frontiers))
        .unwrap();
    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&shallow_snapshot).unwrap();

    let err = shallow_doc
        .diff(&pre_shallow_frontiers, &current_frontiers)
        .unwrap_err();
    assert_eq!(err, LoroError::SwitchToVersionBeforeShallowRoot);
    assert!(!shallow_doc.is_detached());
    assert_eq!(shallow_doc.get_text("t").to_string(), "hello world");

    shallow_doc.get_text("t").insert(11, "!").unwrap();
    shallow_doc.commit();
    assert_eq!(shallow_doc.get_text("t").to_string(), "hello world!");
}

#[test]
fn issue_928_checkout_before_shallow_root_should_error_without_stopping_auto_commit() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();

    let text = doc.get_text("t");
    text.insert(0, "hello").unwrap();
    doc.commit();
    let pre_shallow_frontiers = doc.oplog_frontiers();

    text.insert(5, " world").unwrap();
    doc.commit();
    let current_frontiers = doc.oplog_frontiers();

    let shallow_snapshot = doc
        .export(ExportMode::shallow_snapshot(&current_frontiers))
        .unwrap();
    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&shallow_snapshot).unwrap();

    let err = shallow_doc.checkout(&pre_shallow_frontiers).unwrap_err();
    assert_eq!(err, LoroError::SwitchToVersionBeforeShallowRoot);
    assert!(!shallow_doc.is_detached());
    assert_eq!(shallow_doc.get_text("t").to_string(), "hello world");

    shallow_doc.get_text("t").insert(11, "!").unwrap();
    shallow_doc.commit();
    assert_eq!(shallow_doc.get_text("t").to_string(), "hello world!");
}

#[test]
fn fix_get_unknown_cursor_position() {
    let doc = LoroDoc::new();
    let pos = doc.get_cursor_pos(&Cursor::new(
        None,
        ContainerID::Normal {
            peer: 10,
            counter: 0,
            container_type: loro::ContainerType::List,
        },
        loro::cursor::Side::Left,
        0,
    ));
    assert!(matches!(pos, Err(..)));
}

#[test]
fn issue_924_fork_shallow_snapshot() {
    let doc_a = LoroDoc::new();
    let list_a = doc_a.get_list("list");
    list_a.insert(0, "A").unwrap();
    list_a.insert(1, "B").unwrap();
    list_a.insert(2, "C").unwrap();

    let bytes = doc_a
        .export(ExportMode::shallow_snapshot(&doc_a.oplog_frontiers()))
        .unwrap();

    let doc_b = LoroDoc::new();
    doc_b.import(&bytes).unwrap();

    assert!(doc_b.is_shallow());
    assert!(!doc_b.is_detached());

    let doc_c = doc_b.fork();
    assert!(doc_c.is_shallow());
    assert_eq!(doc_b.get_deep_value(), doc_c.get_deep_value());
}

#[test]
fn get_unknown_cursor_position_but_its_in_pending() {
    let doc_0 = LoroDoc::new();
    let list = doc_0
        .get_map("map")
        .insert_container("list", LoroList::new())
        .unwrap();
    let v = doc_0.oplog_vv();
    let text = list.insert_container(0, LoroText::new()).unwrap();
    text.insert(0, "h").unwrap();
    doc_0.commit();
    text.insert(1, "heihei").unwrap();
    let updates = doc_0.export(ExportMode::updates_owned(v)).unwrap();

    let doc_1 = LoroDoc::new();
    let import_status = doc_1.import(&updates).unwrap();
    assert!(import_status.pending.is_some());
    assert!(doc_1.get_container(text.id()).is_none());
    assert!(!doc_1.has_container(&text.id()));
    assert_eq!(doc_1.get_path_to_container(&text.id()), None);
}

/// Concurrent movable-tree moves must keep the incrementally maintained
/// `DocState` identical to a fresh replay of the full oplog.
///
/// History shape: B creates four roots (B0..B3), then B reorders them
/// (B4..B6) while A — knowing only B0..B3 — creates one root (A0) and
/// reorders everything (A1..A3). A1..A3 are concurrent with B4..B6, but the
/// receiving peer's frontier {B6, A0} is version-included in the merged
/// version, which used to be misclassified as ImportGreaterUpdates: the tree
/// fast path applied A1..A3 without adjudicating against B4..B6 and the
/// materialized state diverged from replay. Run both import orders so either
/// peer can be the one receiving the concurrent branch incrementally.
#[test]
fn tree_concurrent_moves_incremental_state_matches_replay() {
    fn canonical(doc: &LoroDoc) -> LoroDoc {
        let replay = LoroDoc::new();
        replay.get_tree("tree").enable_fractional_index(0);
        replay
            .import(&doc.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        replay.checkout_to_latest();
        replay
    }

    fn run(first_into_b: bool) {
        let a = LoroDoc::new();
        a.set_peer_id(1).unwrap();
        let b = LoroDoc::new();
        b.set_peer_id(2).unwrap();
        a.get_tree("tree").enable_fractional_index(0);
        b.get_tree("tree").enable_fractional_index(0);
        let ta = a.get_tree("tree");
        let tb = b.get_tree("tree");

        let n0 = tb.create(None).unwrap();
        let n1 = tb.create(None).unwrap();
        let n2 = tb.create(None).unwrap();
        let n3 = tb.create(None).unwrap();
        b.commit();
        a.import(&b.export(ExportMode::updates(&a.oplog_vv())).unwrap())
            .unwrap();

        tb.mov_to(n1, None, 0).unwrap();
        tb.mov_to(n2, None, 1).unwrap();
        tb.mov_to(n3, None, 2).unwrap();
        tb.mov_to(n0, None, 3).unwrap();
        b.commit();

        let na = ta.create(None).unwrap();
        a.commit();
        b.import(&a.export(ExportMode::updates(&b.oplog_vv())).unwrap())
            .unwrap();

        ta.mov_to(n3, None, 0).unwrap();
        ta.mov_to(n1, None, 1).unwrap();
        ta.mov_to(n0, None, 2).unwrap();
        ta.mov_to(na, None, 3).unwrap();
        ta.mov_to(n2, None, 4).unwrap();
        a.commit();

        let a_updates = a.export(ExportMode::all_updates()).unwrap();
        let b_updates = b.export(ExportMode::all_updates()).unwrap();
        if first_into_b {
            b.import(&a_updates).unwrap();
            a.import(&b_updates).unwrap();
        } else {
            a.import(&b_updates).unwrap();
            b.import(&a_updates).unwrap();
        }
        a.checkout_to_latest();
        b.checkout_to_latest();

        let replay_a = canonical(&a);
        let replay_b = canonical(&b);
        assert_eq!(a.oplog_vv(), b.oplog_vv());
        assert_eq!(a.get_deep_value(), b.get_deep_value());
        assert_eq!(replay_a.get_deep_value(), replay_b.get_deep_value());
        assert_eq!(a.get_deep_value(), replay_a.get_deep_value());
        assert_eq!(b.get_deep_value(), replay_b.get_deep_value());
    }

    run(true);
    run(false);
}

/// Checkout between divergent versions whose meet is NOT a critical version
/// (diamond: the region op `1@1` is concurrent with the meet head `0@2`).
/// The tree/movable-list/text calculators replay relatively around the base;
/// this pins that such checkouts stay canonical.
#[test]
fn checkout_across_non_critical_meet_stays_canonical() {
    use loro::{Frontiers, TreeID, ID};
    let s = LoroDoc::new();
    s.set_peer_id(9).unwrap();
    s.get_tree("tree").enable_fractional_index(0);
    let ts = s.get_tree("tree");
    let _n = ts.create(None).unwrap();
    let _sib = ts.create(None).unwrap();
    s.commit();
    let prefix = s.export(ExportMode::all_updates()).unwrap();

    let p1 = LoroDoc::new();
    p1.set_peer_id(1).unwrap();
    p1.get_tree("tree").enable_fractional_index(0);
    p1.import(&prefix).unwrap();
    let t1 = p1.get_tree("tree");
    let _m1 = t1.create(None).unwrap();
    t1.mov_to(TreeID::new(9, 0), None, 2).unwrap();
    p1.commit();
    let b1 = p1.export(ExportMode::updates(&s.oplog_vv())).unwrap();

    let p2 = LoroDoc::new();
    p2.set_peer_id(2).unwrap();
    p2.get_tree("tree").enable_fractional_index(0);
    p2.import(&prefix).unwrap();
    let t2 = p2.get_tree("tree");
    t2.mov_to(TreeID::new(9, 0), None, 1).unwrap();
    let _m2 = t2.create(None).unwrap();
    p2.commit();
    let b2 = p2.export(ExportMode::updates(&s.oplog_vv())).unwrap();

    let v1 = Frontiers::from(vec![ID::new(1, 1), ID::new(2, 0)]);
    let v2 = Frontiers::from(vec![ID::new(2, 1), ID::new(1, 0)]);
    let make_full = || {
        let d = LoroDoc::new();
        d.get_tree("tree").enable_fractional_index(0);
        d.import(&prefix).unwrap();
        d.import(&b1).unwrap();
        d.import(&b2).unwrap();
        d
    };
    let ref1 = make_full();
    ref1.checkout(&v1).unwrap();
    let ref2 = make_full();
    ref2.checkout(&v2).unwrap();

    let d = make_full();
    d.checkout(&v1).unwrap();
    assert_eq!(d.get_deep_value(), ref1.get_deep_value());
    d.checkout(&v2).unwrap();
    assert_eq!(d.get_deep_value(), ref2.get_deep_value());
    d.checkout(&v1).unwrap();
    assert_eq!(d.get_deep_value(), ref1.get_deep_value());
}

/// Checkout where the diff region contains a LOW-lamport concurrent branch
/// (below the meet frontier's lamport window). The tree calculator's
/// retreat/forward windows skip ops below `lca_min_lamport`, so this is only
/// safe because the walk retreats the base to a critical version (the
/// latest-singleton-cut sweep) whenever such a branch exists. Pins that
/// interplay: weakening the sweep would silently break this.
#[test]
fn checkout_with_low_lamport_concurrent_branch_stays_canonical() {
    use loro::{Frontiers, TreeID, ID};
    let p = LoroDoc::new();
    p.set_peer_id(9).unwrap();
    p.get_tree("tree").enable_fractional_index(0);
    let tp = p.get_tree("tree");
    let _n = tp.create(None).unwrap();
    p.commit();
    let blob_first = p.export(ExportMode::all_updates()).unwrap();
    for _ in 0..4 {
        tp.create(None).unwrap();
    }
    p.commit();
    let prefix = p.export(ExportMode::all_updates()).unwrap();

    let p3 = LoroDoc::new();
    p3.set_peer_id(3).unwrap();
    p3.get_tree("tree").enable_fractional_index(0);
    p3.import(&blob_first).unwrap();
    let _x = p3.get_tree("tree").create(None).unwrap(); // 0@3, lamport 1
    p3.commit();
    let blob_o = p3.export(ExportMode::updates(&p.oplog_vv())).unwrap();

    let p1 = LoroDoc::new();
    p1.set_peer_id(1).unwrap();
    p1.get_tree("tree").enable_fractional_index(0);
    p1.import(&prefix).unwrap();
    let t1 = p1.get_tree("tree");
    let _m1 = t1.create(None).unwrap();
    t1.mov_to(TreeID::new(9, 0), None, 3).unwrap();
    p1.commit();
    let bx = p1.export(ExportMode::updates(&p.oplog_vv())).unwrap();

    let p2 = LoroDoc::new();
    p2.set_peer_id(2).unwrap();
    p2.get_tree("tree").enable_fractional_index(0);
    p2.import(&prefix).unwrap();
    let t2 = p2.get_tree("tree");
    t2.mov_to(TreeID::new(9, 0), None, 1).unwrap();
    let _m2 = t2.create(None).unwrap();
    p2.commit();
    let by = p2.export(ExportMode::updates(&p.oplog_vv())).unwrap();

    let make_full = || {
        let d = LoroDoc::new();
        d.get_tree("tree").enable_fractional_index(0);
        d.import(&prefix).unwrap();
        d.import(&blob_o).unwrap();
        d.import(&bx).unwrap();
        d.import(&by).unwrap();
        d
    };
    let v1 = Frontiers::from(vec![ID::new(1, 1), ID::new(2, 0), ID::new(3, 0)]);
    let v2 = Frontiers::from(vec![ID::new(2, 1), ID::new(1, 0)]);
    let ref1 = make_full();
    ref1.checkout(&v1).unwrap();
    let ref2 = make_full();
    ref2.checkout(&v2).unwrap();

    let d = make_full();
    d.checkout(&v2).unwrap();
    assert_eq!(d.get_deep_value(), ref2.get_deep_value());
    d.checkout(&v1).unwrap(); // forward the lamport-1 branch across the window
    assert_eq!(d.get_deep_value(), ref1.get_deep_value());
    d.checkout(&v2).unwrap(); // and retreat it again
    assert_eq!(d.get_deep_value(), ref2.get_deep_value());
}

/// https://github.com/loro-dev/loro/issues/1046
///
/// A movable list of containers takes a delete on one peer concurrently with an
/// edit-then-move of the same element on another peer. When the edit and the move
/// arrive as two *separate* update batches, importing the second batch used to
/// panic (`unwrap` on `None` in `movable_list_state.rs`) and poison the doc mutex.
/// Regression introduced by #974; importing both commits as one batch is fine.
#[test]
fn issue_1046_movable_list_delete_vs_edit_then_move_as_two_batches() {
    use loro::{Container, LoroMap, LoroValue, ValueOrContainer};

    fn index_of(d: &LoroDoc, id: &str) -> Option<usize> {
        let l = d.get_movable_list("list");
        (0..l.len()).find(|&i| match l.get(i) {
            Some(ValueOrContainer::Container(Container::Map(m))) => match m.get("id") {
                Some(ValueOrContainer::Value(v)) => v == LoroValue::from(id),
                _ => false,
            },
            _ => false,
        })
    }

    fn map_at(d: &LoroDoc, idx: usize) -> LoroMap {
        match d.get_movable_list("list").get(idx) {
            Some(ValueOrContainer::Container(Container::Map(m))) => m,
            _ => panic!("expected map container"),
        }
    }

    // Base doc: a movable list of two map containers, tagged "a" and "c".
    let base = LoroDoc::new();
    base.set_peer_id(1).unwrap();
    let list = base.get_movable_list("list");
    for tag in ["a", "c"] {
        let m = list.insert_container(list.len(), LoroMap::new()).unwrap();
        m.insert("id", tag).unwrap();
    }
    base.commit();
    let snap = base.export(ExportMode::Snapshot).unwrap();

    let mk = |peer: u64| {
        let d = LoroDoc::new();
        d.import(&snap).unwrap();
        d.set_peer_id(peer).unwrap();
        d.commit();
        d
    };
    let pa = mk(0xA0);
    let pb = mk(0xA1);

    // Peer A: delete "c".
    pa.get_movable_list("list")
        .delete(index_of(&pa, "c").unwrap(), 1)
        .unwrap();
    pa.commit();

    // Peer B, commit 1: edit c's map.
    let v0 = pb.oplog_vv();
    map_at(&pb, index_of(&pb, "c").unwrap())
        .insert("contents", "zombie")
        .unwrap();
    pb.commit();
    let b_edit = pb.export(ExportMode::updates(&v0)).unwrap();

    // Peer B, commit 2: move c to the front.
    let v1 = pb.oplog_vv();
    pb.get_movable_list("list")
        .mov(index_of(&pb, "c").unwrap(), 0)
        .unwrap();
    pb.commit();
    let b_move = pb.export(ExportMode::updates(&v1)).unwrap();

    // Importing the move as a second, separate batch used to panic here.
    pa.import(&b_edit).unwrap();
    pa.import(&b_move).unwrap();

    // Full sync in both directions; peers must converge.
    pb.import(&pa.export(ExportMode::updates(&pb.oplog_vv())).unwrap())
        .unwrap();
    pa.import(&pb.export(ExportMode::updates(&pa.oplog_vv())).unwrap())
        .unwrap();
    assert_eq!(pa.get_deep_value(), pb.get_deep_value());

    // The converged state must match a fresh-doc replay of the same ops.
    let fresh = LoroDoc::new();
    fresh
        .import(&pa.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(fresh.get_deep_value(), pa.get_deep_value());
}
