//! Imports whose new ops are concurrent with the current version only on
//! register containers (map/counter) are replayed from the current version
//! instead of retreating to a critical version. See
//! `crates/loro-internal/docs/diff_calc.md` ("Register-only concurrency").
//!
//! Every test compares against a reference doc that saw the same history in a
//! different order, so the assertions are about convergence, not about the
//! path taken.

use loro::{ExportMode, LoroDoc, LoroMap, LoroText, LoroValue, ToJson, ValueOrContainer};
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::sync::{Arc, Mutex};

fn base_doc(notes: usize) -> LoroDoc {
    let base = LoroDoc::new();
    base.set_peer_id(1).unwrap();
    for i in 0..notes {
        let m = base
            .get_map("notes")
            .insert_container(&format!("b{i}"), LoroMap::new())
            .unwrap();
        m.insert("t", "x").unwrap();
        m.insert_container("body", LoroText::new())
            .unwrap()
            .insert(0, "hi")
            .unwrap();
    }
    base.commit();
    base
}

fn child_map(doc: &LoroDoc, key: &str) -> loro::LoroMap {
    match doc.get_map("notes").get(key).unwrap() {
        ValueOrContainer::Container(c) => c.into_map().unwrap(),
        _ => unreachable!(),
    }
}

fn map_value(doc: &LoroDoc, path: &[&str], key: &str) -> Option<LoroValue> {
    let mut map = doc.get_map(path[0]);
    for step in &path[1..] {
        map = match map.get(step).unwrap() {
            ValueOrContainer::Container(c) => c.into_map().unwrap(),
            _ => unreachable!(),
        };
    }
    match map.get(key) {
        Some(ValueOrContainer::Value(v)) => Some(v),
        Some(ValueOrContainer::Container(_)) => panic!("expected a value"),
        None => None,
    }
}

fn assert_converged(a: &LoroDoc, b: &LoroDoc) {
    assert_eq!(a.oplog_vv(), b.oplog_vv());
    // A root container created locally with `get_*` exists in state before
    // it has ops; an imported doc only materializes it once it has content.
    a.set_hide_empty_root_containers(true);
    b.set_hide_empty_root_containers(true);
    assert_eq!(
        a.get_deep_value().to_json_value(),
        b.get_deep_value().to_json_value()
    );
}

/// The scenario from the perf report: one unmerged concurrent head that only
/// touched a map, then a long chain of imports on the other branch.
#[test]
fn stale_map_head_does_not_force_conservative_replay() {
    let base = base_doc(20);
    let snapshot = base.export(ExportMode::Snapshot).unwrap();

    let p2 = LoroDoc::new();
    p2.set_peer_id(2).unwrap();
    p2.import(&snapshot).unwrap();
    // Padding ops give the stale head's write a greater lamport than the
    // concurrent write in the chain below.
    for i in 0..16 {
        p2.get_map("pad").insert("i", i).unwrap();
        p2.commit();
    }
    child_map(&p2, "b0").insert("t", "from-2").unwrap();
    p2.commit();
    let stale_head = p2.export(ExportMode::updates(&base.oplog_vv())).unwrap();

    let p3 = LoroDoc::new();
    p3.set_peer_id(3).unwrap();
    p3.import(&snapshot).unwrap();
    let mut chain = Vec::new();
    for k in 0..30 {
        let from = p3.oplog_vv();
        let m = p3
            .get_map("notes")
            .insert_container(&format!("c{k}"), LoroMap::new())
            .unwrap();
        m.insert("t", "y").unwrap();
        m.insert_container("body", LoroText::new())
            .unwrap()
            .insert(0, "hello")
            .unwrap();
        // Also write a key the stale head wrote, with a lower lamport than
        // the stale head has: the stale head must keep winning.
        if k == 0 {
            child_map(&p3, "b0").insert("t", "from-3").unwrap();
        }
        p3.commit();
        chain.push(p3.export(ExportMode::updates(&from)).unwrap());
    }

    let receiver = LoroDoc::new();
    receiver.set_peer_id(9).unwrap();
    receiver.import(&snapshot).unwrap();
    receiver.import(&stale_head).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let _sub = receiver.subscribe_root({
        let events = events.clone();
        Arc::new(move |e| {
            for d in e.events {
                events.lock().unwrap().push(d.target.clone());
            }
        })
    });
    for u in &chain {
        receiver.import(u).unwrap();
    }
    assert_eq!(receiver.state_frontiers().len(), 2);

    let reference = LoroDoc::new();
    reference.import(&snapshot).unwrap();
    for u in chain.iter().rev() {
        // pending until deps arrive; exercises the other order
        reference.import(u).unwrap();
    }
    reference.import(&stale_head).unwrap();
    assert_converged(&receiver, &reference);
    assert_eq!(
        map_value(&receiver, &["notes", "b0"], "t"),
        Some(LoroValue::from("from-2")),
        "the stale head has the greater lamport for b0.t"
    );
    let events = events.lock().unwrap();
    assert!(events.iter().any(|t| t.to_string().contains("notes")));
}

/// Same map, same key, both orders of lamport. Also checks that an import
/// that loses the register comparison does not emit an event for the key.
#[test]
fn same_key_register_concurrency_converges_both_ways() {
    for local_wins in [true, false] {
        let base = base_doc(2);
        let snapshot = base.export(ExportMode::Snapshot).unwrap();

        let local = LoroDoc::new();
        local.set_peer_id(2).unwrap();
        local.import(&snapshot).unwrap();
        let remote = LoroDoc::new();
        remote.set_peer_id(3).unwrap();
        remote.import(&snapshot).unwrap();

        // Give one side a longer chain so its lamport is strictly greater.
        let (longer, shorter) = if local_wins {
            (&local, &remote)
        } else {
            (&remote, &local)
        };
        longer.get_map("notes").insert("pad", 1).unwrap();
        longer.commit();
        longer.get_map("notes").insert("k", "long").unwrap();
        longer.commit();
        shorter.get_map("notes").insert("k", "short").unwrap();
        shorter.commit();

        let remote_update = remote
            .export(ExportMode::updates(&base.oplog_vv()))
            .unwrap();
        let before = map_value(&local, &["notes"], "k");
        let changed_keys = Arc::new(Mutex::new(Vec::new()));
        let _sub = local.subscribe_root({
            let changed_keys = changed_keys.clone();
            Arc::new(move |e| {
                for d in e.events {
                    if let loro::event::Diff::Map(m) = &d.diff {
                        for (k, _) in m.updated.iter() {
                            changed_keys.lock().unwrap().push(k.to_string());
                        }
                    }
                }
            })
        });
        local.import(&remote_update).unwrap();

        let reference = LoroDoc::new();
        reference.import(&remote_update).unwrap();
        reference
            .import(&local.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        assert_converged(&local, &reference);
        assert_eq!(
            map_value(&local, &["notes"], "k"),
            Some(LoroValue::from("long"))
        );
        let changed_keys = changed_keys.lock().unwrap();
        if local_wins {
            assert_eq!(map_value(&local, &["notes"], "k"), before);
            assert!(
                !changed_keys.contains(&"k".to_string()),
                "losing import must not report k as changed: {changed_keys:?}"
            );
        } else {
            assert!(changed_keys.contains(&"k".to_string()));
        }
    }
}

/// Persisted state drops the tombstones of a deleted root, so the register
/// diff must come from history, not from comparing against the state.
#[test]
fn deleted_root_tombstone_beats_concurrent_write_after_snapshot() {
    let base = LoroDoc::new();
    base.set_peer_id(1).unwrap();
    base.get_map("gone").insert("value", "base").unwrap();
    base.commit();
    base.delete_root_container(loro::ContainerID::new_root(
        "gone",
        loro::ContainerType::Map,
    ));
    base.commit();

    let target = LoroDoc::new();
    target
        .import(&base.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    // Concurrent local edit on another map so the import below is a
    // multi-head, register-only concurrency.
    target.set_peer_id(4).unwrap();
    target.get_map("other").insert("x", 1).unwrap();
    target.commit();

    let remote = LoroDoc::new();
    remote.set_peer_id(2).unwrap();
    remote.get_map("gone").insert("value", "remote").unwrap();
    remote.commit();
    target
        .import(&remote.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let reference = LoroDoc::new();
    reference
        .import(&remote.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    reference
        .import(&target.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(
        target.get_map("gone").get_deep_value().to_json_value(),
        reference.get_map("gone").get_deep_value().to_json_value()
    );
    assert!(target.get_map("gone").is_empty());
}

/// A change that straddles the current version: only its suffix is new, and
/// that suffix's only causal parent is the implicit predecessor.
#[test]
fn straddling_change_with_register_concurrency_converges() {
    let base = base_doc(3);
    let snapshot = base.export(ExportMode::Snapshot).unwrap();

    let p3 = LoroDoc::new();
    p3.set_peer_id(3).unwrap();
    p3.import(&snapshot).unwrap();
    for i in 0..6 {
        p3.get_map("notes").insert(&format!("k{i}"), i).unwrap();
        child_map(&p3, "b1")
            .get_or_create_container("body", LoroText::new())
            .unwrap();
    }
    p3.commit(); // one change with several ops
    let mut mid = base.oplog_vv();
    mid.set_end(loro::ID::new(3, 3));
    let first_half = p3
        .export(ExportMode::updates_in_range(&[loro::IdSpan::new(3, 0, 3)]))
        .unwrap();
    let second_half = p3.export(ExportMode::updates(&mid)).unwrap();

    let receiver = LoroDoc::new();
    receiver.set_peer_id(2).unwrap();
    receiver.import(&snapshot).unwrap();
    receiver.import(&first_half).unwrap();
    // Concurrent local map write, then the rest of the straddling change.
    receiver.get_map("notes").insert("k1", "local").unwrap();
    receiver.commit();
    receiver.import(&second_half).unwrap();

    let reference = LoroDoc::new();
    reference.import(&snapshot).unwrap();
    reference
        .import(&receiver.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_converged(&receiver, &reference);
    let again = LoroDoc::new();
    again
        .import(&p3.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    again
        .import(&receiver.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_converged(&receiver, &again);
}

/// Detached state (after `import_batch`) catching up to a version that is
/// concurrent with it only on maps.
#[test]
fn checkout_to_latest_with_register_only_concurrency() {
    let base = base_doc(3);
    let snapshot = base.export(ExportMode::Snapshot).unwrap();

    let a = LoroDoc::new();
    a.set_peer_id(2).unwrap();
    a.import(&snapshot).unwrap();
    a.get_map("notes").insert("a", 1).unwrap();
    a.commit();
    let b = LoroDoc::new();
    b.set_peer_id(3).unwrap();
    b.import(&snapshot).unwrap();
    b.get_map("notes").insert("b", 2).unwrap();
    child_map(&b, "b2").insert("t", "b").unwrap();
    b.commit();

    let receiver = LoroDoc::new();
    receiver.import(&snapshot).unwrap();
    receiver
        .import(&a.export(ExportMode::updates(&base.oplog_vv())).unwrap())
        .unwrap();
    receiver.detach();
    receiver
        .import(&b.export(ExportMode::updates(&base.oplog_vv())).unwrap())
        .unwrap();
    assert!(receiver.is_detached());
    receiver.checkout_to_latest();

    let reference = LoroDoc::new();
    reference
        .import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    reference
        .import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_converged(&receiver, &reference);
}

/// Randomized: several peers concurrently editing shared maps, nested maps,
/// texts and lists, syncing in random pairs. Every sync step is checked
/// against a fresh doc that imports the full history in one go, so every
/// harmless/harmful decision is verified by convergence.
#[test]
fn random_multi_peer_sync_converges() {
    for seed in 0..40u64 {
        let mut rng = StdRng::seed_from_u64(seed);
        let base = base_doc(4);
        let snapshot = base.export(ExportMode::Snapshot).unwrap();
        let peers: Vec<LoroDoc> = (0..4)
            .map(|i| {
                let d = LoroDoc::new();
                d.set_peer_id(10 + i).unwrap();
                d.import(&snapshot).unwrap();
                d
            })
            .collect();

        for _step in 0..60 {
            let who = rng.gen_range(0..peers.len());
            let doc = &peers[who];
            match rng.gen_range(0..6) {
                0 => {
                    let key = format!("k{}", rng.gen_range(0..3));
                    doc.get_map("notes")
                        .insert(&key, rng.gen_range(0..100))
                        .unwrap();
                }
                1 => {
                    let note = format!("b{}", rng.gen_range(0..4));
                    child_map(doc, &note)
                        .insert("t", rng.gen_range(0..100))
                        .unwrap();
                }
                2 => {
                    let note = format!("b{}", rng.gen_range(0..4));
                    let text = child_map(doc, &note)
                        .get_or_create_container("body", LoroText::new())
                        .unwrap();
                    let len = text.len_unicode();
                    let pos = rng.gen_range(0..=len);
                    if len > 0 && rng.gen_bool(0.3) {
                        text.delete(pos.min(len - 1), 1).unwrap();
                    } else {
                        text.insert(pos, "ab").unwrap();
                    }
                }
                3 => {
                    let list = doc.get_list("list");
                    let len = list.len();
                    if len > 0 && rng.gen_bool(0.3) {
                        list.delete(rng.gen_range(0..len), 1).unwrap();
                    } else {
                        list.insert(rng.gen_range(0..=len), rng.gen_range(0..100))
                            .unwrap();
                    }
                }
                4 => {
                    let key = format!("n{}", rng.gen_range(0..3));
                    doc.get_map("notes")
                        .insert_container(&key, LoroMap::new())
                        .unwrap()
                        .insert("v", rng.gen_range(0..100))
                        .unwrap();
                }
                _ => {
                    doc.commit();
                    let from = rng.gen_range(0..peers.len());
                    let to = rng.gen_range(0..peers.len());
                    if from != to {
                        let update = peers[from]
                            .export(ExportMode::updates(&peers[to].oplog_vv()))
                            .unwrap();
                        peers[to].import(&update).unwrap();
                        let reference = LoroDoc::new();
                        reference.import(&snapshot).unwrap();
                        reference
                            .import(&peers[to].export(ExportMode::all_updates()).unwrap())
                            .unwrap();
                        assert_converged(&peers[to], &reference);
                    }
                }
            }
        }

        for p in &peers {
            p.commit();
        }
        let all: Vec<_> = peers
            .iter()
            .map(|p| p.export(ExportMode::all_updates()).unwrap())
            .collect();
        for p in &peers {
            for u in &all {
                p.import(u).unwrap();
            }
        }
        for p in &peers[1..] {
            assert_converged(&peers[0], p);
        }
    }
}
