//! Retreats (undo, checkout) served from the list trackers, checked against
//! fresh replicas and recorded states.

use loro::{
    ExportMode, Frontiers, LoroDoc, LoroList, LoroMap, LoroMovableList, LoroValue, ToJson,
    UndoManager,
};

fn lcg(seed: &mut u64) -> f64 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*seed >> 11) as f64) / ((1u64 << 53) as f64)
}

fn below(seed: &mut u64, n: usize) -> usize {
    (lcg(seed) * n as f64) as usize
}

/// A fresh replica replays the whole history: the oracle.
fn assert_matches_fresh_replica(doc: &LoroDoc) {
    let fresh = LoroDoc::new();
    fresh
        .import(&doc.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        fresh.get_deep_value().to_json_value()
    );
}

fn edit(seed: &mut u64, step: usize, list: &LoroList, movable: &LoroMovableList) {
    let n = list.len();
    match below(seed, 4) {
        0 if n > 0 => list.delete(below(seed, n), 1).unwrap(),
        1 => {
            let map = list
                .insert_container(below(seed, n + 1), LoroMap::new())
                .unwrap();
            map.insert("step", step as i64).unwrap();
        }
        _ => list.insert(below(seed, n + 1), step as i64).unwrap(),
    }
    let n = movable.len();
    match below(seed, 6) {
        0 if n > 0 => movable.delete(below(seed, n), 1).unwrap(),
        1 if n > 1 => movable.mov(below(seed, n), below(seed, n)).unwrap(),
        2 if n > 0 => movable.set(below(seed, n), -(step as i64)).unwrap(),
        3 => {
            let map = movable
                .insert_container(below(seed, n + 1), LoroMap::new())
                .unwrap();
            map.insert("step", step as i64).unwrap();
        }
        _ => movable.insert(below(seed, n + 1), step as i64).unwrap(),
    }
}

#[test]
fn checkout_walks_back_through_list_history() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let list = doc.get_list("l");
    let movable = doc.get_movable_list("m");
    let mut seed = 7u64;
    let mut stops: Vec<(Frontiers, LoroValue)> = vec![];
    for step in 0..80 {
        for _ in 0..1 + below(&mut seed, 3) {
            edit(&mut seed, step, &list, &movable);
        }
        doc.commit();
        stops.push((doc.oplog_frontiers(), doc.get_deep_value()));
    }

    for (frontiers, value) in stops.iter().rev().chain(stops.iter()) {
        doc.checkout(frontiers).unwrap();
        assert_eq!(&doc.get_deep_value(), value);
    }
    doc.checkout_to_latest();
    assert_matches_fresh_replica(&doc);
}

#[test]
fn undo_of_a_commit_that_makes_many_lists() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.get_map("root").insert("title", "paths").unwrap();
    doc.commit();
    let mut undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    let before = doc.get_deep_value();

    let paths = doc
        .get_map("root")
        .insert_container("paths", LoroList::new())
        .unwrap();
    for p in 0..40 {
        let path = paths.push_container(LoroMap::new()).unwrap();
        path.insert("closed", p % 2 == 0).unwrap();
        let anchors = path
            .insert_container("anchors", LoroMovableList::new())
            .unwrap();
        for k in 0..8 {
            let anchor = anchors.push_container(LoroMap::new()).unwrap();
            anchor.insert("x", (p * 10 + k) as f64).unwrap();
            anchor.insert("y", k as f64).unwrap();
        }
        anchors.mov(0, 7).unwrap();
    }
    doc.commit();
    let after = doc.get_deep_value();

    assert!(undo.undo().unwrap());
    assert_eq!(doc.get_deep_value(), before);
    assert_matches_fresh_replica(&doc);
    assert!(undo.redo().unwrap());
    assert_eq!(doc.get_deep_value(), after);
    assert_matches_fresh_replica(&doc);
}

#[test]
fn undo_with_concurrent_peers_converges() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    let mut seed = 11u64;
    for round in 0..60 {
        for (doc, d) in [(&a, 0), (&b, 1)] {
            let list = doc.get_list("l");
            let movable = doc.get_movable_list("m");
            for _ in 0..1 + below(&mut seed, 3) {
                edit(&mut seed, round * 2 + d, &list, &movable);
            }
            doc.commit();
        }
        match below(&mut seed, 4) {
            0 => {
                undo.undo().unwrap();
            }
            1 => {
                undo.redo().unwrap();
            }
            _ => {}
        }
        if round % 2 == 0 {
            a.import(&b.export(ExportMode::updates(&a.oplog_vv())).unwrap())
                .unwrap();
            b.import(&a.export(ExportMode::updates(&b.oplog_vv())).unwrap())
                .unwrap();
            assert_eq!(a.get_deep_value(), b.get_deep_value(), "round {round}");
            assert_matches_fresh_replica(&a);
        }
    }
}
