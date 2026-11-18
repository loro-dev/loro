//! High-signal invariant, randomized oracle, and regression tests for the
//! local-edit fast paths (`MapHandler` unchanged-write skip under txn lock,
//! `subscribe_root` queue & panic safety, movable-list batch `Move`+`Set`,
//! richtext cursor cache & ASCII/UTF-16/Unicode parity, and grouped undo without checkouts).

use std::collections::BTreeMap;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use loro_internal::configure::{StyleConfig, StyleConfigMap};
use loro_internal::container::richtext::ExpandType;
use loro_internal::cursor::PosType;
use loro_internal::event::Index;
use loro_internal::handler::TextDelta;
use loro_internal::loro::ExportMode;
use loro_internal::{LoroDoc, LoroError, LoroValue, MemoryBudget, MemoryLimitKind, UndoManager};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn gen_range(&mut self, upper: usize) -> usize {
        if upper == 0 {
            0
        } else {
            (self.next() as usize) % upper
        }
    }
}

#[test]
fn auto_commit_commit_and_budget_invariants() {
    let doc = LoroDoc::new();
    doc.start_auto_commit();
    doc.set_change_merge_interval(0);
    assert!(matches!(
        doc.txn().unwrap_err(),
        LoroError::DuplicatedTransactionError
    ));

    doc.set_peer_id(42).unwrap();
    assert_eq!(doc.peer_id(), 42);

    let seen_origin = Arc::new(std::sync::Mutex::new(Vec::new()));
    let origins = Arc::clone(&seen_origin);
    let _sub = doc.subscribe_root(Arc::new(move |ev| {
        origins.lock().unwrap().push(ev.event_meta.origin.clone());
    }));

    // Explicit empty commit swallows options (`origin`, `commit_msg`, `timestamp`).
    doc.set_next_commit_origin("swallowed-origin");
    doc.set_next_commit_message("swallowed-msg");
    doc.commit_then_renew();

    let text = doc.get_text("t");
    text.insert_unicode(0, "a").unwrap();
    doc.commit_then_renew();
    assert_eq!(seen_origin.lock().unwrap().as_slice(), &["".into()]);
    let json = doc
        .export_json_updates(&Default::default(), &doc.oplog_vv(), false)
        .unwrap();
    assert_eq!(json.changes.len(), 1);
    assert_eq!(json.changes[0].msg, None);

    // Implicit barrier commit on an empty auto-commit txn preserves options across the barrier.
    doc.set_next_commit_origin("barrier-origin");
    doc.set_next_commit_message("barrier-msg");
    let _ = doc.export(ExportMode::Snapshot).unwrap();
    text.insert_unicode(1, "bc").unwrap();
    doc.commit_then_renew();
    assert_eq!(
        seen_origin.lock().unwrap().as_slice(),
        &["".into(), "barrier-origin".into()]
    );
    let json = doc
        .export_json_updates(&Default::default(), &doc.oplog_vv(), false)
        .unwrap();
    assert_eq!(json.changes.len(), 2);
    assert_eq!(json.changes[1].msg.as_deref(), Some("barrier-msg"));
    assert!(matches!(
        doc.txn().unwrap_err(),
        LoroError::DuplicatedTransactionError
    ));

    // Arm DocBudget and ensure `try_commit` rolls back.
    let tight = doc.memory_stats().total_bytes + 128;
    doc.set_memory_budget(MemoryBudget::new().with_max_doc_bytes(tight));
    text.insert_unicode(3, &"z".repeat(8_000)).unwrap();
    let err = doc.try_commit().unwrap_err();
    assert_eq!(
        err.memory_limit_info().map(|i| i.kind),
        Some(MemoryLimitKind::DocBudget)
    );
    assert_eq!(text.to_string(), "abc");
}

#[test]
fn unchanged_map_write_and_deleted_or_unmatched_errors() {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let map = doc.get_map("m");
    map.insert("k", 1_i64).unwrap();
    doc.commit_then_renew();
    let vv_after_first = doc.oplog_vv();

    // Identical write on a live map key skips emitting an op.
    map.insert("k", 1_i64).unwrap();
    doc.commit_then_renew();
    assert_eq!(doc.oplog_vv(), vv_after_first);

    // Identical write on a deleted child map must still return `ContainerDeleted`.
    let child = map
        .insert_container("child", loro_internal::MapHandler::new_detached())
        .unwrap();
    child.insert("x", 7_i64).unwrap();
    doc.commit_then_renew();
    map.delete("child").unwrap();
    doc.commit_then_renew();
    assert!(matches!(
        child.insert("x", 7_i64).unwrap_err(),
        LoroError::ContainerDeleted { .. }
    ));

    // Mismatched-doc `insert_with_txn` must fail cleanly without leaking state to `txn_b`.
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2).unwrap();
    let map_b = doc_b.get_map("m");
    let mut txn_b = doc_b.txn().unwrap();
    map_b
        .insert_with_txn(&mut txn_b, "k", 10_i64.into())
        .unwrap();
    txn_b.commit().unwrap();

    let mut txn_b2 = doc_b.txn().unwrap();
    assert!(matches!(
        map.insert_with_txn(&mut txn_b2, "k", 1_i64.into())
            .unwrap_err(),
        LoroError::UnmatchedContext { .. }
    ));
    map_b
        .insert_with_txn(&mut txn_b2, "k", 20_i64.into())
        .unwrap();
    txn_b2.commit().unwrap();
    assert_eq!(map_b.get("k"), Some(LoroValue::I64(20)));
}

#[test]
fn root_only_subscriber_queue_and_panic_safety() {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let nested = doc
        .get_map("m")
        .insert_container("child", loro_internal::TextHandler::new_detached())
        .unwrap();
    doc.commit_then_renew();

    let hits = Arc::new(AtomicUsize::new(0));
    let keys = Arc::new(std::sync::Mutex::new(Vec::new()));
    let hits_cb = Arc::clone(&hits);
    let keys_cb = Arc::clone(&keys);
    let doc_clone = doc.clone();
    let nested_cb = nested.clone();
    let sub = doc.subscribe_root(Arc::new(move |ev| {
        for diff in ev.events {
            for (_, index) in &diff.path {
                if let Index::Key(k) = index {
                    keys_cb.lock().unwrap().push(k.to_string());
                }
            }
        }
        if hits_cb.fetch_add(1, Ordering::SeqCst) == 0 {
            nested_cb.insert_unicode(0, "b").unwrap();
            doc_clone.commit_then_renew();
        }
    }));
    nested.insert_unicode(0, "a").unwrap();
    doc.commit_then_renew();
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    assert_eq!(nested.to_string(), "ba");
    assert!(keys.lock().unwrap().iter().any(|k| k == "child"));
    drop(sub);

    // Panicking root subscriber must not drop `subscribe_local_update` bytes.
    let local_bytes = Arc::new(AtomicUsize::new(0));
    let lb = Arc::clone(&local_bytes);
    let _local_sub = doc.subscribe_local_update(Box::new(move |bytes| {
        lb.fetch_add(bytes.len(), Ordering::SeqCst);
        true
    }));
    let _panic_sub = doc.subscribe_root(Arc::new(|_| panic!("boom in root callback")));
    nested.insert_unicode(2, "c").unwrap();
    let res = std::panic::catch_unwind(AssertUnwindSafe(|| doc.commit_then_renew()));
    assert!(res.is_err());
    assert!(local_bytes.load(Ordering::SeqCst) > 0);
}

#[test]
fn movable_list_move_then_set_in_same_import_batch_updates_value() {
    let p1 = LoroDoc::new_auto_commit();
    p1.set_peer_id(1).unwrap();
    let ml1 = p1.get_movable_list("ml");
    ml1.push("zero".into()).unwrap();
    ml1.push("one".into()).unwrap();
    p1.commit_then_renew();
    let base = p1.export(ExportMode::all_updates()).unwrap();

    let p2 = LoroDoc::new();
    p2.set_peer_id(2).unwrap();
    p2.import(&base).unwrap();
    let p2_base_vv = p2.oplog_vv();
    {
        let mut txn = p2.txn().unwrap();
        let ml2 = p2.get_movable_list("ml");
        ml2.move_with_txn(&mut txn, 0, 1).unwrap();
        ml2.set_with_txn(&mut txn, 1, "zero-updated".into())
            .unwrap();
        txn.commit().unwrap();
    }
    let p2_updates = p2.export(ExportMode::updates(&p2_base_vv)).unwrap();

    // Concurrent edit forces `MovableListDiffCalculator` across p2's Move+Set batch.
    ml1.push("two".into()).unwrap();
    p1.commit_then_renew();
    p1.import(&p2_updates).unwrap();

    let oracle = LoroDoc::new();
    oracle.detach();
    oracle
        .import(&p1.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    oracle.checkout_to_latest();
    assert_eq!(p1.get_deep_value(), oracle.get_deep_value());
}

#[test]
fn randomized_richtext_cursor_cache_and_ascii_unicode_utf16_oracle() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let mut styles = StyleConfigMap::new();
    styles.insert("bold".into(), StyleConfig::new().expand(ExpandType::After));
    doc.config_text_style(styles);
    let text = doc.get_text("t");
    let mut expected = String::new();
    let tokens = ["a", "xyz", "你好", "😀", "b🦀c", " \n"];

    for step in 0..250 {
        let cur_unicode = expected.chars().count();
        assert_eq!(text.len_unicode(), cur_unicode);
        assert_eq!(text.len_utf16(), expected.encode_utf16().count());
        assert_eq!(text.len_utf8(), expected.len());

        let op_choice = if cur_unicode == 0 {
            0
        } else {
            rng.gen_range(10)
        };
        if op_choice < 5 {
            let tok = tokens[rng.gen_range(tokens.len())];
            let u_pos = rng.gen_range(cur_unicode + 1);
            let b_pos = expected
                .char_indices()
                .nth(u_pos)
                .map(|(i, _)| i)
                .unwrap_or(expected.len());
            let unit_kind = rng.gen_range(3);
            match unit_kind {
                0 => text.insert(u_pos, tok, PosType::Unicode).unwrap(),
                1 => text
                    .insert(
                        expected[..b_pos].encode_utf16().count(),
                        tok,
                        PosType::Utf16,
                    )
                    .unwrap(),
                _ => text.insert(b_pos, tok, PosType::Bytes).unwrap(),
            }
            expected.insert_str(b_pos, tok);
        } else if op_choice < 9 {
            let u_pos = rng.gen_range(cur_unicode);
            let max_del = (cur_unicode - u_pos).min(4);
            let u_len = rng.gen_range(max_del) + 1;
            let b_start = expected.char_indices().nth(u_pos).unwrap().0;
            let b_end = expected
                .char_indices()
                .nth(u_pos + u_len)
                .map(|(i, _)| i)
                .unwrap_or(expected.len());
            if rng.gen_range(2) == 0 {
                text.delete(u_pos, u_len, PosType::Unicode).unwrap();
            } else {
                let utf16_start = expected[..b_start].encode_utf16().count();
                let utf16_len = expected[b_start..b_end].encode_utf16().count();
                text.delete(utf16_start, utf16_len, PosType::Utf16).unwrap();
            }
            expected.replace_range(b_start..b_end, "");
        } else if cur_unicode >= 2 {
            let start = rng.gen_range(cur_unicode - 1);
            let end = start + 1 + rng.gen_range((cur_unicode - start).min(4));
            let bold = rng.gen_range(2) == 0;
            text.mark(start, end, "bold", LoroValue::Bool(bold), PosType::Unicode)
                .unwrap();
        }
        if step % 3 != 0 {
            doc.commit_then_renew();
        }
        assert_eq!(text.to_string(), expected);
    }
    doc.commit_then_renew();

    let oracle = LoroDoc::new();
    oracle
        .import(&doc.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(text.to_string(), expected);
    assert_eq!(
        text.get_richtext_value(),
        oracle.get_text("t").get_richtext_value()
    );
    let delta_chars: usize = text
        .get_delta()
        .into_iter()
        .map(|d| match d {
            TextDelta::Insert { insert, .. } => insert.chars().count(),
            _ => unreachable!(),
        })
        .sum();
    assert_eq!(delta_chars, expected.chars().count());
}

#[test]
fn randomized_grouped_undo_redo_matches_oracle_without_checkout_regression() {
    // Part 1: grouped pure text insertions + map writes on cold snapshot must undo WITHOUT expanding cold history blocks.
    let seed_doc = LoroDoc::new_auto_commit();
    seed_doc.set_peer_id(1).unwrap();
    let seed_text = seed_doc.get_text("t");
    for i in 0..40 {
        seed_text.insert_unicode(i, "x").unwrap();
        seed_doc.commit_then_renew();
    }
    seed_text.insert_unicode(40, "abcd").unwrap();
    seed_doc.commit_then_renew();
    let snap = seed_doc.export(ExportMode::Snapshot).unwrap();

    let cold = LoroDoc::new();
    cold.import(&snap).unwrap();
    cold.set_peer_id(2).unwrap();
    let undo = UndoManager::new(&cold);
    undo.set_merge_interval(0);
    let t = cold.get_text("t");
    let m = cold.get_map("m");
    let before = t.to_string();
    let before_val = cold.get_deep_value();
    undo.group_start().unwrap();
    t.insert_unicode(41, "X").unwrap();
    cold.commit_then_renew();
    t.insert_unicode(42, "Y").unwrap();
    cold.commit_then_renew();
    m.insert("k", 1_i64).unwrap();
    cold.commit_then_renew();
    undo.group_end();
    let after = t.to_string();
    let after_val = cold.get_deep_value();
    assert!(after.ends_with("aXYbcd"));
    let stats_before_undo = cold.memory_stats();
    assert!(undo.undo().unwrap());
    assert_eq!(t.to_string(), before);
    assert_eq!(cold.get_deep_value(), before_val);
    let stats_after_undo = cold.memory_stats();
    let verify_env = std::env::var("LORO_VERIFY_UNDO_LOG")
        .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
    if !verify_env {
        assert_eq!(
            stats_after_undo.diff_calc_bytes, stats_before_undo.diff_calc_bytes,
            "grouped undo must not populate diff_calculator via checkout"
        );
    }
    assert!(undo.redo().unwrap());
    assert_eq!(t.to_string(), after);
    assert_eq!(cold.get_deep_value(), after_val);

    // Part 2: randomized multi-container undo/redo sequence under LORO_VERIFY_UNDO_LOG=1.
    let mut rng = Rng(0x517c_c1b7_2722_0a95);
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    let mut snapshots = BTreeMap::new();
    let t = doc.get_text("t");
    let list = doc.get_list("l");
    let map = doc.get_map("m");
    #[cfg(feature = "counter")]
    let counter = doc.get_counter("c");
    snapshots.insert(0usize, doc.get_deep_value());

    let mut pushed_steps = 0usize;
    for step in 1..=80 {
        let grouped = rng.gen_range(3) == 0;
        if grouped {
            undo.group_start().unwrap();
        }
        let sub_ops = if grouped { 1 + rng.gen_range(3) } else { 1 };
        for _ in 0..sub_ops {
            match rng.gen_range(7) {
                0..=1 => {
                    let pos = rng.gen_range(t.len_unicode() + 1);
                    let s = ["a", "bc", "😀"][rng.gen_range(3)];
                    t.insert_unicode(pos, s).unwrap();
                }
                2 => {
                    let n = t.len_unicode();
                    if n > 0 {
                        let pos = rng.gen_range(n);
                        t.delete_unicode(pos, 1).unwrap();
                    }
                }
                3 => {
                    let pos = rng.gen_range(list.len() + 1);
                    list.insert(pos, step as i64).unwrap();
                }
                4 => {
                    let n = list.len();
                    if n > 0 {
                        let pos = rng.gen_range(n);
                        list.delete(pos, 1).unwrap();
                    }
                }
                5 => {
                    let k = ["k0", "k1", "k2"][rng.gen_range(3)];
                    if rng.gen_range(5) == 0 {
                        map.delete(k).unwrap();
                    } else {
                        map.insert(k, rng.gen_range(10) as i64).unwrap();
                    }
                }
                _ => {
                    #[cfg(feature = "counter")]
                    counter.increment(1.5).unwrap();
                    #[cfg(not(feature = "counter"))]
                    t.insert_unicode(0, "q").unwrap();
                }
            }
            doc.commit_then_renew();
        }
        if grouped {
            undo.group_end();
        }
        if undo.undo_count() > pushed_steps {
            pushed_steps = undo.undo_count();
            snapshots.insert(pushed_steps, doc.get_deep_value());
        }
    }

    let max_depth = undo.undo_count();
    let final_snapshot = doc.get_deep_value();
    let mut rem = max_depth;
    while undo.can_undo() {
        let _ = undo.undo().unwrap();
        rem -= 1;
        assert_eq!(doc.get_deep_value(), *snapshots.get(&rem).unwrap());
    }
    assert_eq!(rem, 0);
    let redo_steps = undo.redo_count();
    assert_eq!(redo_steps, max_depth);
    for step in 1..=redo_steps {
        assert!(undo.can_redo());
        let _ = undo.redo().unwrap();
        assert_eq!(doc.get_deep_value(), *snapshots.get(&step).unwrap());
    }
    assert!(!undo.can_redo());
    assert_eq!(doc.get_deep_value(), final_snapshot);
}
