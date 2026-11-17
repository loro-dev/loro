//! A quota-rejected import is undone with the import undo log
//! (`state/undo_log.rs`), not by computing an inverse diff from history. These
//! tests compare the rejected document with a fork taken before the import:
//! same value and version, and the same results for the same later edits and
//! imports (which also depend on internal ids and positions).

use rand::{rngs::StdRng, Rng, SeedableRng};

use crate::{
    cursor::PosType, handler::HandlerTrait, loro::ExportMode, ContainerType, LoroDoc, LoroValue,
    MemoryBudget, TreeParentId,
};

fn random_edits(doc: &LoroDoc, rng: &mut StdRng, n: usize) {
    for _ in 0..n {
        match rng.gen_range(0..14) {
            0 | 1 => {
                let text = doc.get_text("t");
                let pos = rng.gen_range(0..=text.len_unicode());
                let s: String = (0..rng.gen_range(1..6))
                    .map(|_| rng.gen_range(b'a'..=b'z') as char)
                    .collect();
                text.insert(pos, &s, PosType::Unicode).unwrap();
            }
            2 => {
                let text = doc.get_text("t");
                let len = text.len_unicode();
                if len > 1 {
                    let pos = rng.gen_range(0..len - 1);
                    let del = rng.gen_range(1..=(len - pos).min(4));
                    text.delete(pos, del, PosType::Unicode).unwrap();
                }
            }
            3 => {
                let text = doc.get_text("t");
                let len = text.len_unicode();
                if len > 2 {
                    let start = rng.gen_range(0..len - 1);
                    let end = rng.gen_range(start + 1..=len);
                    let key = ["bold", "link"][rng.gen_range(0..2)];
                    let value = if rng.gen_bool(0.2) {
                        LoroValue::Null
                    } else {
                        LoroValue::Bool(true)
                    };
                    let _ = text.mark(start, end, key, value, PosType::Unicode);
                }
            }
            4 => {
                let list = doc.get_list("l");
                let pos = rng.gen_range(0..=list.len());
                list.insert(pos, rng.gen_range(0..100)).unwrap();
            }
            5 => {
                let list = doc.get_list("l");
                if !list.is_empty() {
                    let pos = rng.gen_range(0..list.len());
                    let del = rng.gen_range(1..=(list.len() - pos).min(3));
                    list.delete(pos, del).unwrap();
                }
            }
            6 => {
                let map = doc.get_map("m");
                let key = format!("k{}", rng.gen_range(0..6));
                match rng.gen_range(0..4) {
                    0 => map.delete(&key).unwrap(),
                    1 => {
                        let child = map
                            .insert_container(&key, crate::MapHandler::new_detached())
                            .unwrap();
                        child.insert("x", rng.gen_range(0..10)).unwrap();
                    }
                    _ => map.insert(&key, rng.gen_range(0..100)).unwrap(),
                }
            }
            7 | 8 => {
                let ml = doc.get_movable_list("ml");
                let len = ml.len();
                match rng.gen_range(0..4) {
                    0 if len > 1 => {
                        let from = rng.gen_range(0..len);
                        let to = rng.gen_range(0..len);
                        ml.mov(from, to).unwrap();
                    }
                    1 if len > 0 => {
                        ml.set(rng.gen_range(0..len), rng.gen_range(0..100))
                            .unwrap();
                    }
                    2 if len > 0 => {
                        let pos = rng.gen_range(0..len);
                        ml.delete(pos, 1).unwrap();
                    }
                    _ => ml
                        .insert(rng.gen_range(0..=len), rng.gen_range(0..100))
                        .unwrap(),
                }
            }
            9 | 10 => {
                let tree = doc.get_tree("tr");
                // Sorted: `nodes` follows hash map order, which depends on history.
                let mut nodes = tree.nodes();
                nodes.sort();
                match rng.gen_range(0..3) {
                    0 if !nodes.is_empty() => {
                        let target = nodes[rng.gen_range(0..nodes.len())];
                        let parent = if rng.gen_bool(0.3) {
                            TreeParentId::Root
                        } else {
                            TreeParentId::Node(nodes[rng.gen_range(0..nodes.len())])
                        };
                        let _ = tree.mov(target, parent);
                    }
                    1 if !nodes.is_empty() => {
                        let target = nodes[rng.gen_range(0..nodes.len())];
                        let _ = tree.delete(target);
                    }
                    _ => {
                        let parent = if nodes.is_empty() || rng.gen_bool(0.4) {
                            TreeParentId::Root
                        } else {
                            TreeParentId::Node(nodes[rng.gen_range(0..nodes.len())])
                        };
                        if let Ok(node) = tree.create(parent) {
                            let meta = tree.get_meta(node).unwrap();
                            let _ = meta.insert("v", rng.gen_range(0..10));
                        }
                    }
                }
            }
            #[cfg(feature = "counter")]
            11 => {
                let counter = doc.get_counter("c");
                // Halves add up exactly in any order, so peers that apply the
                // same increments in different orders still compare equal.
                counter
                    .increment(rng.gen_range(0..10) as f64 + 0.5)
                    .unwrap();
            }
            _ => {
                let text = doc.get_text("t");
                let pos = rng.gen_range(0..=text.len_unicode());
                text.insert(pos, "Z", PosType::Unicode).unwrap();
            }
        }
    }
    doc.commit_then_renew();
}

fn non_empty_roots(value: LoroValue) -> LoroValue {
    let LoroValue::Map(map) = value else {
        unreachable!()
    };
    let map: crate::FxHashMap<String, LoroValue> = map
        .iter()
        .filter(|(_, v)| match v {
            LoroValue::List(l) => !l.is_empty(),
            LoroValue::Map(m) => !m.is_empty(),
            LoroValue::String(s) => !s.is_empty(),
            _ => true,
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    LoroValue::Map(map.into())
}

fn fork_at_same_peer(doc: &LoroDoc) -> LoroDoc {
    let fork = doc.fork();
    fork.set_peer_id(doc.peer_id()).unwrap();
    fork
}

fn assert_same(doc: &LoroDoc, reference: &LoroDoc, what: &str) {
    assert_eq!(doc.get_deep_value(), reference.get_deep_value(), "{what}");
    assert_eq!(doc.oplog_frontiers(), reference.oplog_frontiers(), "{what}");
    assert_eq!(doc.state_frontiers(), reference.state_frontiers(), "{what}");
}

/// Rejected imports, single and batch, of random concurrent edits to every
/// container type, compared with a fork that never saw them.
#[test]
fn a_rejected_import_undone_by_the_log_matches_a_fork_that_never_imported() {
    let mut rejected = 0;
    for seed in 0..40u64 {
        let mut rng = StdRng::seed_from_u64(seed);
        let src = LoroDoc::new_auto_commit();
        src.set_peer_id(1).unwrap();
        let dst = LoroDoc::new_auto_commit();
        dst.set_peer_id(2).unwrap();
        random_edits(&src, &mut rng, 30);
        dst.import(&src.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        for round in 0..6 {
            let (n_src, n_dst) = (rng.gen_range(1..25), rng.gen_range(0..10));
            random_edits(&src, &mut rng, n_src);
            random_edits(&dst, &mut rng, n_dst);
            let update = src.export(ExportMode::updates(&dst.oplog_vv())).unwrap();
            let reference = fork_at_same_peer(&dst);
            // Usually a quota nothing fits, so the import is rejected; sometimes
            // one just above usage, which reclaims (demotes states) first.
            let limit = if rng.gen_bool(0.8) {
                1
            } else {
                dst.memory_stats().total_bytes + 64
            };
            dst.set_memory_budget(MemoryBudget {
                max_doc_bytes: Some(limit),
                ..Default::default()
            });
            let use_batch = rng.gen_bool(0.3);
            let res = if use_batch {
                dst.import_batch(&[update.clone()]).map(|_| ())
            } else {
                dst.import(&update).map(|_| ())
            };
            dst.set_memory_budget(MemoryBudget::default());
            let what = format!("seed {seed} round {round} batch {use_batch}");
            match res {
                Err(err) => {
                    assert!(err.memory_limit_info().is_some(), "{what}: {err}");
                    rejected += 1;
                    assert_same(&dst, &reference, &what);
                }
                Ok(()) => {
                    reference.import(&update).unwrap();
                    assert_same(&dst, &reference, &what);
                }
            }
            // The same later edits and imports give the same results.
            let follow_seed = rng.gen();
            random_edits(&dst, &mut StdRng::seed_from_u64(follow_seed), 8);
            random_edits(&reference, &mut StdRng::seed_from_u64(follow_seed), 8);
            assert_eq!(
                dst.get_deep_value(),
                reference.get_deep_value(),
                "{what}: after the same local edits"
            );
            dst.import(&update).unwrap();
            reference.import(&update).unwrap();
            assert_eq!(
                dst.get_deep_value(),
                reference.get_deep_value(),
                "{what}: after importing the update"
            );
            src.import(&dst.export(ExportMode::updates(&src.oplog_vv())).unwrap())
                .unwrap();
            // Root visibility differs by design (a root a peer acquired a handle
            // to shows even when empty; context/root-container-visibility.md),
            // also without any quota, so compare the non-empty roots.
            assert_eq!(
                non_empty_roots(src.get_deep_value()),
                non_empty_roots(dst.get_deep_value()),
                "{what}: synced"
            );
        }
    }
    assert!(rejected > 100, "too few rejections exercised: {rejected}");
    let _ = ContainerType::Text;
}

/// Rolling back reads no history: only the forward diff is computed.
#[test]
fn rolling_back_a_rejected_import_computes_no_diff() {
    let src = LoroDoc::new_auto_commit();
    src.set_peer_id(1).unwrap();
    let text = src.get_text("t");
    text.insert(0, "hello world", PosType::Unicode).unwrap();
    src.commit_then_renew();
    let dst = LoroDoc::new_auto_commit();
    dst.import(&src.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let vv = src.oplog_vv();
    text.delete(2, 5, PosType::Unicode).unwrap();
    text.insert(1, &"x".repeat(4096), PosType::Unicode).unwrap();
    src.commit_then_renew();
    let update = src.export(ExportMode::updates(&vv)).unwrap();
    dst.set_memory_budget(MemoryBudget {
        max_doc_bytes: Some(dst.memory_stats().total_bytes + 64),
        ..Default::default()
    });
    let before = dst.get_deep_value();
    crate::diff_calc::DIFF_CALCS.with(|n| n.set(0));
    assert!(dst.import(&update).is_err());
    assert_eq!(crate::diff_calc::DIFF_CALCS.with(|n| n.get()), 1);
    assert_eq!(dst.get_deep_value(), before);
}
