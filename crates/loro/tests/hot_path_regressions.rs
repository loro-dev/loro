#[cfg(feature = "jsonpath")]
use loro::{Container, LoroMap, ValueOrContainer};
use loro::{LoroDoc, LoroText, UndoManager};
#[cfg(feature = "jsonpath")]
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Barrier,
};

#[test]
#[cfg(feature = "jsonpath")]
fn shared_child_handles_commit_once_when_the_last_two_drop_concurrently() {
    for _ in 0..200 {
        let doc = LoroDoc::new();
        let root = doc.get_map("root");
        root.insert_container("child", LoroMap::new()).unwrap();
        doc.commit();
        let first = match doc.jsonpath("$.root.*").unwrap().pop().unwrap() {
            ValueOrContainer::Container(Container::Map(map)) => map,
            _ => unreachable!(),
        };
        let second = first.clone();
        let updates = Arc::new(AtomicUsize::new(0));
        let observed = updates.clone();
        doc.subscribe_local_update(Box::new(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            true
        }))
        .detach();
        first.insert("value", 1).unwrap();
        drop(root);
        drop(doc);
        assert_eq!(updates.load(Ordering::SeqCst), 0);
        let barrier = Arc::new(Barrier::new(2));
        let other_barrier = barrier.clone();
        let worker = std::thread::spawn(move || {
            other_barrier.wait();
            drop(first);
        });
        barrier.wait();
        drop(second);
        worker.join().unwrap();
        assert_eq!(updates.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn grouped_text_inserts_preserve_creation_and_unicode_across_undo_redo() {
    let doc = LoroDoc::new();
    let root = doc.get_map("root");
    let mut undo = UndoManager::new(&doc);
    undo.group_start().unwrap();
    let text = root.insert_container("text", LoroText::new()).unwrap();
    doc.commit();
    for content in ["汉", "😀", "字", "e\u{301}"] {
        text.insert(text.len_unicode(), content).unwrap();
        doc.commit();
    }
    undo.group_end();
    let after = doc.get_deep_value();
    assert_eq!(text.to_string(), "汉😀字e\u{301}");
    assert!(undo.undo().unwrap());
    assert!(root.get("text").is_none());
    assert!(undo.redo().unwrap());
    assert_eq!(doc.get_deep_value(), after);
}

#[test]
fn inserted_runs_preserve_middle_insertions_and_intervening_deletions() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "outside").unwrap();
    doc.commit();
    let mut undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    text.insert(3, "汉😀").unwrap();
    text.insert(4, "abc").unwrap();
    text.insert(3, "前").unwrap();
    text.insert(9, "尾").unwrap();
    doc.commit();
    let after_insert = text.to_string();
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "outside");
    assert!(undo.redo().unwrap());
    assert_eq!(text.to_string(), after_insert);
    text.insert(0, "first").unwrap();
    text.delete(2, 2).unwrap();
    text.insert(2, "second").unwrap();
    doc.commit();
    let after_mixed = text.to_string();
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), after_insert);
    assert!(undo.redo().unwrap());
    assert_eq!(text.to_string(), after_mixed);
}

#[test]
fn cloning_handle_concurrently_during_another_handle_drop_preserves_final_commit() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };

    for _ in 0..100 {
        let doc = LoroDoc::new();
        let updates = Arc::new(AtomicUsize::new(0));
        let observed = updates.clone();
        doc.subscribe_local_update(Box::new(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            true
        }))
        .detach();

        let map1 = doc.get_map("m");
        let map2 = map1.clone();
        drop(doc);
        map1.insert("k1", 1).unwrap();

        let barrier = Arc::new(Barrier::new(2));
        let b2 = barrier.clone();
        let worker = std::thread::spawn(move || {
            b2.wait();
            let map3 = map2.clone();
            drop(map2);
            map3
        });
        barrier.wait();
        drop(map1);
        let surviving = worker.join().unwrap();
        assert_eq!(updates.load(Ordering::SeqCst), 0);

        surviving.insert("k2", 2).unwrap();
        drop(surviving);
        assert_eq!(updates.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn snapshot_load_and_subsequent_commits_keep_cold_peer_blocks_unparsed() {
    use loro::{CommitOptions, ExportMode, ID};

    const PEERS: u64 = 128;
    let hub = LoroDoc::new();
    for peer in 1..=PEERS {
        let p = LoroDoc::new();
        p.set_peer_id(peer).unwrap();
        let payload = format!("peer-{peer:04}-{}", "x".repeat(4096));
        p.get_map("map")
            .insert("payload", payload.as_str())
            .unwrap();
        p.commit_with(CommitOptions::new().timestamp(1_000 + peer as i64 * 10));
        hub.import(&p.export(ExportMode::all_updates()).unwrap())
            .unwrap();
    }
    let snapshot = hub.export(ExportMode::Snapshot).unwrap();
    let expected_max_ts = 1_000 + PEERS as i64 * 10;

    let restored = LoroDoc::new();
    restored.set_peer_id(9_999).unwrap();
    restored.import(&snapshot).unwrap();

    let stats_after_import = restored.memory_stats();
    assert_eq!(
        stats_after_import.change_cache_bytes, 0,
        "snapshot import must not parse cold frontier blocks into mem_parsed_kv"
    );
    assert!(
        stats_after_import.dag_bytes < 16 * 1024,
        "snapshot import must keep DAG nodes lazy: got {} bytes",
        stats_after_import.dag_bytes
    );

    // Committing a local edit (with timestamp lower than max frontier timestamp, plus a
    // second edit merged within merge_interval) must compute exact lamport & monotonic
    // timestamp in O(1) without expanding the 128 cold 4 KiB peer blocks.
    restored.set_change_merge_interval(1_000);
    restored.get_map("map").insert("local", "v1").unwrap();
    restored.commit_with(CommitOptions::new().timestamp(500));

    let first_change = restored.get_change(ID::new(9_999, 0)).unwrap();
    assert_eq!(first_change.lamport, 1);
    assert_eq!(first_change.timestamp, expected_max_ts);

    // Merge into previous local change with timestamp +10 (within merge_interval = 1000),
    // then force a separate change (via commit_msg) with a lower explicit timestamp to
    // verify stored timestamp tracking matches `last.timestamp` rather than the merged input.
    restored.get_map("map").insert("local", "v2").unwrap();
    restored.commit_with(CommitOptions::new().timestamp(expected_max_ts + 10));
    let merged_change = restored.get_change(ID::new(9_999, 1)).unwrap();
    assert_eq!(merged_change.timestamp, expected_max_ts);

    restored.get_map("map").insert("local", "v3").unwrap();
    restored.commit_with(CommitOptions::new().timestamp(1).commit_msg("separate"));
    let third_change = restored.get_change(ID::new(9_999, 2)).unwrap();
    assert_eq!(third_change.lamport, 3);
    assert_eq!(third_change.timestamp, expected_max_ts);

    let stats_after_commits = restored.memory_stats();
    assert!(
        stats_after_commits.change_cache_bytes < 16 * 1024,
        "local commits after snapshot load must not parse cold peer blocks: got {} bytes",
        stats_after_commits.change_cache_bytes
    );

    let mirror = LoroDoc::new();
    mirror
        .import(&restored.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(mirror.get_deep_value(), restored.get_deep_value());
}

#[test]
fn unknown_span_partial_delete_and_checkout_retreat_preserves_real_ids() {
    use loro::ExportMode;

    let base = LoroDoc::new();
    base.set_peer_id(1).unwrap();
    base.get_text("text")
        .insert(0, "0123456789abcdefghij")
        .unwrap();
    let list = base.get_list("list");
    for i in 0..20 {
        list.push(i).unwrap();
    }
    base.commit();
    let f_base = base.oplog_frontiers();
    let snapshot = base.export(ExportMode::Snapshot).unwrap();

    // Load from snapshot so the base content starts below any tracker replay base,
    // exercising `UNKNOWN_PEER_ID` spans and out-of-line `unknown_real_ids` across
    // non-adjacent deletes, concurrent splits, and checkout retreats.
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(2).unwrap();
    doc_a.import(&snapshot).unwrap();
    doc_a.get_text("text").delete(2, 6).unwrap();
    doc_a.get_text("text").delete(8, 4).unwrap();
    for _ in 0..4 {
        doc_a.get_list("list").delete(3, 1).unwrap();
    }
    doc_a.commit();
    let f_after_del = doc_a.oplog_frontiers();

    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(3).unwrap();
    doc_b.import(&snapshot).unwrap();
    doc_b.get_text("text").insert(5, "XYZ").unwrap();
    doc_b.get_list("list").insert(5, 999).unwrap();
    doc_b.commit();

    doc_a
        .import(&doc_b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    doc_b
        .import(&doc_a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(doc_a.get_deep_value(), doc_b.get_deep_value());
    let merged_value = doc_a.get_deep_value();

    doc_a.checkout(&f_after_del).unwrap();
    doc_a.checkout(&f_base).unwrap();
    assert_eq!(doc_a.get_deep_value(), base.get_deep_value());
    doc_a.try_checkout_to_latest().unwrap();
    assert_eq!(doc_a.get_deep_value(), merged_value);
}
