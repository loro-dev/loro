//! Checkouts must not depend on the path taken.
//!
//! Several peers make concurrent edits (tree create/move/delete, moves into
//! and out of deleted subtrees, metadata edits, plus map/list/movable
//! list/text/counter edits) and sync at random. Then one document walks
//! through every ordered pair `(a, b)` of the collected versions:
//! `checkout(a)`, then `checkout(b)`. After each checkout its value must equal
//! a fresh document that imported only the changes of that version, and the
//! value rebuilt from its events must equal its state. The versions include
//! heads that no peer ever had, so the transitions cross concurrent branches.
//!
//! The seeds are fixed; `CHECKOUT_PATH_SEEDS=start..end` runs others. The
//! `loro.js` differential fuzz found the tree cases (loro-dev/loro#1141): the
//! other fuzzers only check out versions from one peer's own history, which
//! are ordered by inclusion. See `context/tree-checkout-window.md`.

use fuzz::actor::{assert_value_eq, Actor};
use loro::{
    ExportMode, Frontiers, LoroDoc, LoroMap, LoroText, LoroTree, LoroValue, TreeID, TreeParentId,
    ID,
};
use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};

const PEERS: u64 = 3;
const ACTIONS: usize = 40;
const MAX_VERSIONS: usize = 24;

fn random_parent(tree: &LoroTree, rng: &mut StdRng) -> TreeParentId {
    let nodes = tree.nodes();
    if nodes.is_empty() || rng.gen_bool(0.3) {
        TreeParentId::Root
    } else {
        TreeParentId::Node(*nodes.choose(rng).unwrap())
    }
}

fn random_node(tree: &LoroTree, rng: &mut StdRng) -> Option<TreeID> {
    tree.nodes().choose(rng).copied()
}

fn random_edit(doc: &LoroDoc, rng: &mut StdRng) {
    let tree = doc.get_tree("tree");
    let value = rng.gen_range(0..100);
    // Errors are expected: moving a node below itself, editing a deleted
    // node's metadata, out-of-range indexes after a sync, and so on.
    let _ = match rng.gen_range(0..16) {
        0..=2 => {
            let parent = random_parent(&tree, rng);
            tree.create(parent).map(|_| ())
        }
        3..=5 => match random_node(&tree, rng) {
            // Includes moves into deleted subtrees and moves that revive
            // nodes under deleted ancestors.
            Some(target) => {
                let parent = random_parent(&tree, rng);
                tree.mov(target, parent)
            }
            None => Ok(()),
        },
        6 => match random_node(&tree, rng) {
            Some(target) => tree.delete(target),
            None => Ok(()),
        },
        7..=8 => match random_node(&tree, rng) {
            Some(target) => tree
                .get_meta(target)
                .and_then(|meta| meta.insert(&format!("k{}", value % 3), value)),
            None => Ok(()),
        },
        9 => match random_node(&tree, rng) {
            Some(target) => tree.get_meta(target).and_then(|meta| {
                meta.insert_container("text", LoroText::new())?
                    .insert(0, "t")
            }),
            None => Ok(()),
        },
        10 => {
            let map = doc.get_map("map");
            if rng.gen_bool(0.3) {
                map.delete(&format!("k{}", value % 3))
            } else if rng.gen_bool(0.3) {
                map.insert_container(&format!("k{}", value % 3), LoroMap::new())
                    .and_then(|m| m.insert("v", value))
            } else {
                map.insert(&format!("k{}", value % 3), value)
            }
        }
        11 => {
            let list = doc.get_list("list");
            if !list.is_empty() && rng.gen_bool(0.4) {
                list.delete(rng.gen_range(0..list.len()), 1)
            } else {
                list.insert(rng.gen_range(0..=list.len()), value)
            }
        }
        12 => {
            let list = doc.get_movable_list("movable");
            let len = list.len();
            match rng.gen_range(0..4) {
                0 if len > 1 => list.mov(rng.gen_range(0..len), rng.gen_range(0..len)),
                1 if len > 0 => list.delete(rng.gen_range(0..len), 1),
                2 if len > 0 => list.set(rng.gen_range(0..len), value),
                3 => list
                    .insert_container(rng.gen_range(0..=len), LoroMap::new())
                    .and_then(|m| m.insert("v", value)),
                _ => list.insert(rng.gen_range(0..=len), value),
            }
        }
        13 => {
            let text = doc.get_text("text");
            let len = text.len_unicode();
            if len > 0 && rng.gen_bool(0.3) {
                let pos = rng.gen_range(0..len);
                text.delete(pos, 1)
            } else {
                text.insert(rng.gen_range(0..=len), "ab")
            }
        }
        14 => doc.get_counter("counter").increment(value as f64),
        _ => {
            // A burst of local edits gives this peer higher lamports than the
            // others, so concurrent branches end up with very different
            // lamport ranges.
            (0..3).try_for_each(|_| {
                doc.get_map("burst").insert("k", value)?;
                doc.commit();
                Ok(())
            })
        }
    };
    doc.commit();
}

fn random_version(doc: &LoroDoc, rng: &mut StdRng) -> Frontiers {
    let vv = doc.oplog_vv();
    let mut ids = Vec::new();
    for _ in 0..rng.gen_range(1..=2) {
        let peers: Vec<_> = vv.iter().filter(|(_, end)| **end > 0).collect();
        let Some((peer, end)) = peers.choose(rng) else {
            break;
        };
        ids.push(ID::new(**peer, rng.gen_range(0..**end)));
    }
    let vv = doc.frontiers_to_vv(&Frontiers::from(ids)).unwrap();
    doc.vv_to_frontiers(&vv)
}

fn value_at(all: &LoroDoc, version: &Frontiers) -> LoroValue {
    let vv = all.frontiers_to_vv(version).unwrap();
    let doc = LoroDoc::new();
    doc.set_hide_empty_root_containers(true);
    doc.import(&all.export(ExportMode::updates_till(&vv)).unwrap())
        .unwrap();
    doc.get_deep_value()
}

fn check_checkout(doc: &LoroDoc, version: &Frontiers, expected: &LoroValue, context: &str) {
    doc.checkout(version).unwrap();
    assert_value_eq(
        expected,
        &doc.get_deep_value(),
        Some(&mut || format!("{context}, checkout to {version:?}")),
    );
}

fn run(seed: u64) {
    let mut rng = StdRng::seed_from_u64(seed);
    let peers: Vec<LoroDoc> = (1..=PEERS)
        .map(|peer| {
            let doc = LoroDoc::new();
            doc.set_peer_id(peer).unwrap();
            doc
        })
        .collect();
    for _ in 0..ACTIONS {
        let i = rng.gen_range(0..peers.len());
        if rng.gen_bool(0.25) {
            let j = rng.gen_range(0..peers.len());
            if i != j {
                let update = peers[j]
                    .export(ExportMode::updates(&peers[i].oplog_vv()))
                    .unwrap();
                peers[i].import(&update).unwrap();
            }
        } else {
            random_edit(&peers[i], &mut rng);
        }
    }

    let all = LoroDoc::new();
    for peer in peers.iter() {
        all.import(&peer.export(ExportMode::all_updates()).unwrap())
            .unwrap();
    }
    let mut versions = vec![Frontiers::default(), all.oplog_frontiers()];
    while versions.len() < MAX_VERSIONS {
        let version = random_version(&all, &mut rng);
        if !versions.contains(&version) {
            versions.push(version);
        }
    }
    let expected: Vec<LoroValue> = versions.iter().map(|v| value_at(&all, v)).collect();

    // `Actor` mirrors the document from its events.
    let with_events = Actor::new(100);
    let plain = LoroDoc::new();
    let updates = all.export(ExportMode::all_updates()).unwrap();
    for doc in [with_events.loro.as_ref(), &plain] {
        doc.set_hide_empty_root_containers(true);
        doc.import(&updates).unwrap();
    }
    with_events.check_tracker();
    for (a, b) in (0..versions.len()).flat_map(|a| (0..versions.len()).map(move |b| (a, b))) {
        for (i, context) in [(a, "from"), (b, "to")] {
            let context = format!(
                "seed {seed}, {context} of {:?} -> {:?}",
                versions[a], versions[b]
            );
            check_checkout(&with_events.loro, &versions[i], &expected[i], &context);
            with_events.check_tracker();
            check_checkout(&plain, &versions[i], &expected[i], &context);
        }
    }
}

#[test]
fn checkout_between_any_two_versions_is_path_independent() {
    let seeds: Vec<u64> = match std::env::var("CHECKOUT_PATH_SEEDS") {
        Ok(range) => {
            let (start, end) = range
                .split_once("..")
                .expect("CHECKOUT_PATH_SEEDS=start..end");
            (start.parse().unwrap()..end.parse().unwrap()).collect()
        }
        Err(_) => (0..24).collect(),
    };
    let failed: Vec<u64> = seeds
        .into_iter()
        .filter(|&seed| std::panic::catch_unwind(|| run(seed)).is_err())
        .collect();
    assert!(failed.is_empty(), "failed seeds: {failed:?}");
}
