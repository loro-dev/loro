//! Differential & atomicity tests for the direct linear import fast path.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use loro_internal::{
    cursor::PosType,
    handler::{HandlerTrait, ListHandler, MapHandler, TextHandler},
    loro::ExportMode,
    LoroDoc, LoroError, LoroValue, MemoryBudget, MemoryLimitKind, TreeParentId,
};

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

fn export_all(doc: &LoroDoc) -> Vec<u8> {
    doc.export(ExportMode::all_updates()).unwrap()
}

fn assert_all_three_agree(updates: &[Vec<u8>]) {
    let quiet = LoroDoc::new();
    let subscribed = LoroDoc::new();
    let event_count = Arc::new(AtomicUsize::new(0));
    let count_cb = Arc::clone(&event_count);
    let _sub = subscribed.subscribe_root(Arc::new(move |_| {
        count_cb.fetch_add(1, Ordering::SeqCst);
    }));
    let detached = LoroDoc::new();
    detached.detach();

    for update in updates {
        quiet.import(update).unwrap();
        subscribed.import(update).unwrap();
        detached.import(update).unwrap();
    }
    detached.checkout_to_latest();

    assert!(event_count.load(Ordering::SeqCst) > 0);
    assert_eq!(quiet.oplog_vv(), detached.oplog_vv());
    assert_eq!(quiet.state_frontiers(), quiet.oplog_frontiers());
    assert_eq!(quiet.get_deep_value(), detached.get_deep_value());
    assert_eq!(subscribed.get_deep_value(), detached.get_deep_value());
    assert_eq!(
        quiet.get_text("t").get_richtext_value(),
        detached.get_text("t").get_richtext_value()
    );
    assert_eq!(
        subscribed.get_text("t").get_richtext_value(),
        detached.get_text("t").get_richtext_value()
    );
}

#[test]
fn randomized_multi_peer_linear_relay_matches_checkout_and_events() {
    let mut rng = Rng(0x243f_6a88_85a3_08d3);
    let src = LoroDoc::new_auto_commit();
    let peers = [1_u64, 2, 3];
    let mut updates = Vec::new();
    let mut child_maps: Vec<MapHandler> = Vec::new();
    let mut child_lists: Vec<ListHandler> = Vec::new();

    for step in 0..80 {
        src.set_peer_id(peers[rng.gen_range(peers.len())]).unwrap();
        let before = src.oplog_vv();
        match rng.gen_range(12) {
            0..=2 => {
                let text = src.get_text("t");
                let pos = rng.gen_range(text.len_unicode() + 1);
                let chunk = ["hi ", "世界", "😀!", "abc"][rng.gen_range(4)];
                text.insert(pos, chunk, PosType::Unicode).unwrap();
            }
            3 => {
                let text = src.get_text("t");
                let n = text.len_utf16();
                if n > 0 {
                    let pos = rng.gen_range(n);
                    let len = (1 + rng.gen_range(2)).min(n - pos);
                    let _ = text.delete(pos, len, PosType::Utf16);
                }
            }
            4 => {
                let text = src.get_text("t");
                if text.len_unicode() >= 2 && step > 20 {
                    let start = rng.gen_range(text.len_unicode() - 1);
                    let end = start + 1;
                    text.mark(start, end, "bold", LoroValue::Bool(true), PosType::Unicode)
                        .unwrap();
                } else {
                    text.insert(0, "s", PosType::Unicode).unwrap();
                }
            }
            5 => {
                let list = src.get_list("l");
                let pos = rng.gen_range(list.len() + 1);
                if rng.gen_range(5) == 0 {
                    let h = list
                        .insert_container(pos, MapHandler::new_detached())
                        .unwrap();
                    child_maps.push(h);
                } else {
                    list.insert(pos, LoroValue::I64(step as i64)).unwrap();
                }
            }
            6 => {
                let list = src.get_list("l");
                if !list.is_empty() {
                    let pos = rng.gen_range(list.len());
                    list.delete(pos, 1).unwrap();
                }
            }
            7 => {
                let map = src.get_map("m");
                let k = ["a", "b", "c"][rng.gen_range(3)];
                match rng.gen_range(6) {
                    0 => {
                        let h = map
                            .insert_container(k, ListHandler::new_detached())
                            .unwrap();
                        child_lists.push(h);
                    }
                    1 => {
                        map.insert(k, LoroValue::Binary(vec![1, 2, 3].into()))
                            .unwrap();
                    }
                    2 => {
                        map.delete(k).unwrap();
                    }
                    _ => {
                        map.insert(k, LoroValue::I64(step as i64)).unwrap();
                    }
                }
            }
            8 => {
                if let Some(m) = child_maps.get(rng.gen_range(child_maps.len().max(1))) {
                    let _ = m.insert("nested", step as i64);
                }
                if let Some(l) = child_lists.get(rng.gen_range(child_lists.len().max(1))) {
                    let _ = l.push(step as i64);
                }
                src.get_text("plain")
                    .insert(0, "z", PosType::Unicode)
                    .unwrap();
            }
            9 => {
                #[cfg(feature = "counter")]
                src.get_counter("cnt").increment(1.25).unwrap();
                #[cfg(not(feature = "counter"))]
                src.get_text("plain")
                    .insert(0, "c", PosType::Unicode)
                    .unwrap();
            }
            10 => {
                let tree = src.get_tree("tr");
                let parent = TreeParentId::Root;
                tree.create(parent).unwrap();
            }
            _ => {
                let ml = src.get_movable_list("ml");
                ml.push(LoroValue::I64(step as i64)).unwrap();
                if ml.len() >= 2 {
                    ml.mov(0, ml.len() - 1).unwrap();
                }
            }
        }
        src.commit_then_renew();
        updates.push(src.export(ExportMode::updates(&before)).unwrap());
    }

    assert_all_three_agree(&updates);
}

#[test]
fn direct_import_skips_dangling_child_container_and_preserves_atomicity() {
    // 1. Deleted child container ops in a linear update fall back safely and match checkout.
    let src = LoroDoc::new_auto_commit();
    src.set_peer_id(1).unwrap();
    let root_map = src.get_map("m");
    let child_text = root_map
        .insert_container("child", TextHandler::new_detached())
        .unwrap();
    child_text.insert_unicode(0, "hello").unwrap();
    src.commit_then_renew();
    let child_id = child_text.id();
    let u1 = export_all(&src);
    let vv1 = src.oplog_vv();

    root_map.delete("child").unwrap();
    src.commit_then_renew();
    let u2 = src.export(ExportMode::updates(&vv1)).unwrap();
    let vv2 = src.oplog_vv();

    // Another peer keeps editing the child before seeing the delete, or creates a causal write
    // targeting a deleted container via detached/imported op: create peer 2 with child alive,
    // wait — linear extension on peer 2 after `u2` can't edit `child_text` directly because
    // local `insert` rejects `ContainerDeleted`, so import an update containing a write to a
    // deleted child created concurrently on peer 2 with causal dep on `u2` via `src` before delete,
    // plus verify attached `[u1, u2]` matches checkout cleanly.
    let p2 = LoroDoc::new_auto_commit();
    p2.set_peer_id(2).unwrap();
    p2.import(&u1).unwrap();
    let vv_p2 = p2.oplog_vv();
    let p2_child = p2.get_text(child_id.clone());
    p2_child.insert_unicode(5, " world").unwrap();
    p2.commit_then_renew();
    let u3 = p2.export(ExportMode::updates(&vv_p2)).unwrap();
    let _ = vv2;
    assert_all_three_agree(&[u1.clone(), u2.clone(), u3]);

    let attached_deleted = LoroDoc::new();
    attached_deleted.import(&u1).unwrap();
    assert!(attached_deleted.get_path_to_container(&child_id).is_some());
    attached_deleted.import(&u2).unwrap();
    assert!(attached_deleted.get_path_to_container(&child_id).is_none());

    // 2. Truncated update leaves doc unchanged.
    let mut bad = u1.clone();
    bad.truncate(4);
    let doc = LoroDoc::new();
    let err = doc.import(&bad).unwrap_err();
    assert!(matches!(err, LoroError::DecodeError(_)), "{err:?}");
    assert!(doc.oplog_frontiers().is_empty());

    // 3. Linear append followed by DocBudget rejection without a subscriber rolls back cleanly.
    let a = LoroDoc::new_auto_commit();
    a.set_peer_id(1).unwrap();
    a.get_text("t").insert_unicode(0, "base").unwrap();
    a.commit_then_renew();
    let base = export_all(&a);

    let b = LoroDoc::new_auto_commit();
    b.set_peer_id(2).unwrap();
    b.import(&base).unwrap();
    let vv = b.oplog_vv();
    b.get_text("t").insert_unicode(4, " tail").unwrap();
    b.commit_then_renew();
    let extra = b.export(ExportMode::updates(&vv)).unwrap();

    let target = LoroDoc::new();
    target.import(&base).unwrap();
    target.import(&extra).unwrap();
    assert_eq!(target.get_text("t").to_string(), "base tail");

    let tight = target.memory_stats().total_bytes + 256;
    target.set_memory_budget(MemoryBudget::new().with_max_doc_bytes(tight));
    let big_src = LoroDoc::new_auto_commit();
    big_src.set_peer_id(9).unwrap();
    big_src
        .get_text("big")
        .insert_unicode(0, &"x".repeat(20_000))
        .unwrap();
    big_src.commit_then_renew();
    let err = target.import(&export_all(&big_src)).unwrap_err();
    assert_eq!(
        err.memory_limit_info().map(|i| i.kind),
        Some(MemoryLimitKind::DocBudget)
    );
    assert_eq!(target.get_text("t").to_string(), "base tail");
}

#[test]
fn direct_import_preserves_compact_lazy_text_state_memory() {
    // 1. Large append base + 50 alternating-peer append updates must stay in compact LazyLoad::Src.
    let src = LoroDoc::new_auto_commit();
    src.set_peer_id(1).unwrap();
    src.get_text("text")
        .insert_unicode(0, &"a".repeat(50_000))
        .unwrap();
    src.commit_then_renew();
    let base = export_all(&src);
    let mut updates = Vec::with_capacity(50);
    for i in 0..50 {
        src.set_peer_id(if i % 2 == 0 { 2 } else { 1 }).unwrap();
        let vv = src.oplog_vv();
        let text = src.get_text("text");
        text.insert_unicode(text.len_unicode(), "x").unwrap();
        src.commit_then_renew();
        updates.push(src.export(ExportMode::updates(&vv)).unwrap());
    }
    let expected = src.get_text("text").to_string();

    let imported = LoroDoc::new();
    imported.import(&base).unwrap();
    for u in &updates {
        imported.import(u).unwrap();
    }
    assert_eq!(imported.get_text("text").to_string(), expected);
    let stats = imported.memory_stats();
    assert!(
        stats.state_bytes < 200_000,
        "append direct import materialized InnerState: state_bytes={}",
        stats.state_bytes
    );

    // 2. Mixed insert + delete imported into an empty text falls back to DiffCalculator so LazyLoad::Src stays compact.
    let churn = LoroDoc::new_auto_commit();
    churn.set_peer_id(1).unwrap();
    let t = churn.get_text("text");
    t.insert_unicode(0, &"a".repeat(50_000)).unwrap();
    churn.commit_then_renew();
    t.delete_unicode(10, 100).unwrap();
    churn.commit_then_renew();
    t.insert_unicode(10, "xyz").unwrap();
    churn.commit_then_renew();
    let churn_blob = export_all(&churn);

    let fresh = LoroDoc::new();
    fresh.import(&churn_blob).unwrap();
    assert_eq!(fresh.get_text("text").to_string(), t.to_string());
    let stats2 = fresh.memory_stats();
    assert!(
        stats2.state_bytes < 200_000,
        "initial mixed import materialized InnerState: state_bytes={}",
        stats2.state_bytes
    );
}
