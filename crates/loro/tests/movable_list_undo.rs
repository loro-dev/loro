use loro::{ContainerTrait, ExportMode, LoroDoc, LoroText, ToJson, UndoManager};
use serde_json::{json, Value};

fn pair(values: &[i32]) -> (LoroDoc, LoroDoc) {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    for (i, x) in values.iter().enumerate() {
        a.get_movable_list("list").insert(i, *x).unwrap();
    }
    a.commit();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    send(&a, &b);
    (a, b)
}

fn send(a: &LoroDoc, b: &LoroDoc) {
    b.import_with(&a.export(ExportMode::all_updates()).unwrap(), "import")
        .unwrap();
}

fn sync(a: &LoroDoc, b: &LoroDoc) {
    send(a, b);
    send(b, a);
}

fn undo(a: &LoroDoc) -> UndoManager {
    let mut u = UndoManager::new(a);
    u.add_exclude_origin_prefix("import");
    u.set_merge_interval(0);
    u
}

fn list(a: &LoroDoc) -> Value {
    a.get_movable_list("list").get_value().to_json_value()
}

fn state(a: &LoroDoc) -> Value {
    a.get_deep_value().to_json_value()
}

fn converge(a: &LoroDoc, b: &LoroDoc) {
    sync(a, b);
    assert_eq!(state(a), state(b));
    for mode in [ExportMode::Snapshot, ExportMode::all_updates()] {
        let restored = LoroDoc::new();
        restored.import(&a.export(mode).unwrap()).unwrap();
        assert_eq!(state(a), state(&restored));
    }
}

#[test]
fn undo_same_element_concurrent_move() {
    let (a, b) = pair(&[0, 1, 2, 3]);
    let mut u = undo(&a);
    a.get_movable_list("list").mov(0, 2).unwrap();
    a.commit();
    b.get_movable_list("list").mov(0, 1).unwrap();
    b.commit();
    sync(&a, &b);
    let before = list(&a);
    let r = u.undo();
    assert_eq!(r, Ok(true));
    let after = list(&a);
    println!("CASE1 before={before} undo={r:?} after={after}");
    assert_eq!(before, json!([1, 0, 2, 3]));
    assert_eq!(after, json!([0, 1, 2, 3]));
    sync(&a, &b);
    assert_eq!(state(&a), state(&b));
    for _ in 0..3 {
        assert!(u.redo().unwrap());
        assert_eq!(list(&a), before);
        converge(&a, &b);
        assert!(u.undo().unwrap());
        assert_eq!(list(&a), json!([0, 1, 2, 3]));
        converge(&a, &b);
    }
    // This set still names the original element on B. Undo must have moved it,
    // rather than deleting it and inserting an equal value with a new identity.
    b.get_movable_list("list").set(0, 99).unwrap();
    b.commit();
    converge(&a, &b);
    assert_eq!(list(&a), json!([99, 1, 2, 3]));
}

#[test]
fn undo_removal_uses_surviving_element_anchors() {
    // Left neighbour wins if the old neighbours cross. If it disappears, use
    // the nearest surviving right neighbour, then the start of an empty list.
    let cases = [
        (0, json!([2, 3, 0, 1])),
        (1, json!([2, 0, 1, 3])),
        (2, json!([1, 2, 3])),
        (3, json!([1, 3])),
        (4, json!([1])),
        (5, json!([0, 1, 9, 2, 3])),
    ];
    for (case, expected) in cases {
        let (a, b) = pair(&[0, 1, 2, 3]);
        let mut u = undo(&a);
        a.get_movable_list("list").delete(1, 1).unwrap();
        a.commit();
        let remote = b.get_movable_list("list");
        match case {
            0 => remote.mov(0, 3).unwrap(),
            1 => remote.mov(2, 0).unwrap(),
            2 => remote.delete(0, 1).unwrap(),
            3 => {
                remote.delete(2, 1).unwrap();
                remote.delete(0, 1).unwrap();
            }
            4 => {
                remote.delete(2, 2).unwrap();
                remote.delete(0, 1).unwrap();
            }
            5 => remote.insert(1, 9).unwrap(),
            _ => unreachable!(),
        }
        b.commit();
        converge(&a, &b);
        let removed = list(&a);
        assert!(u.undo().unwrap(), "case {case}");
        assert_eq!(list(&a), expected, "case {case}");
        converge(&a, &b);
        assert!(u.redo().unwrap());
        assert_eq!(list(&a), removed, "case {case}");
        converge(&a, &b);
    }
}

#[test]
fn undo_insertion_follows_identity_after_remote_move_with_equal_values() {
    let (a, b) = pair(&[7, 7]);
    let mut u = undo(&a);
    a.get_movable_list("list").insert(1, 7).unwrap();
    a.commit();
    send(&a, &b);
    b.get_movable_list("list").mov(1, 2).unwrap();
    b.commit();
    converge(&a, &b);
    assert!(u.undo().unwrap());
    assert_eq!(list(&a), json!([7, 7]));
    // Edit the two original identities before receiving the undo. Comparing
    // payloads would allow the wrong equal-valued element to be deleted.
    b.get_movable_list("list").set(0, 10).unwrap();
    b.get_movable_list("list").set(1, 20).unwrap();
    b.commit();
    converge(&a, &b);
    assert_eq!(list(&a), json!([10, 20]));
}

#[test]
fn undo_move_does_not_revive_remotely_deleted_element() {
    let (a, b) = pair(&[0, 1, 2, 3]);
    let mut u = undo(&a);
    a.get_movable_list("list").mov(0, 2).unwrap();
    a.commit();
    send(&a, &b);
    b.get_movable_list("list").delete(2, 1).unwrap();
    b.commit();
    converge(&a, &b);
    let version = a.oplog_vv();
    assert_eq!(u.undo_count(), 1);
    assert!(!u.undo().unwrap());
    assert_eq!(a.oplog_vv(), version);
    assert_eq!(u.undo_count(), 0);
    assert_eq!(list(&a), json!([1, 2, 3]));
    converge(&a, &b);
}

#[test]
fn older_steps_follow_recreated_scalar_identities() {
    let (a, b) = pair(&[0, 1]);
    let mut u = undo(&a);
    a.get_movable_list("list").insert(1, 7).unwrap();
    a.commit();
    a.get_movable_list("list").delete(1, 1).unwrap();
    a.commit();
    for _ in 0..3 {
        assert!(u.undo().unwrap());
        assert_eq!(list(&a), json!([0, 7, 1]));
        assert!(u.undo().unwrap());
        assert_eq!(list(&a), json!([0, 1]));
        assert!(u.redo().unwrap());
        assert_eq!(list(&a), json!([0, 7, 1]));
        assert!(u.redo().unwrap());
        assert_eq!(list(&a), json!([0, 1]));
        converge(&a, &b);
    }
}

#[test]
fn removal_anchors_follow_recreated_neighbour_identities() {
    let (a, b) = pair(&[0, 1, 2, 3]);
    let mut u = undo(&a);
    a.get_movable_list("list").delete(1, 1).unwrap();
    a.commit();
    a.get_movable_list("list").delete(0, 1).unwrap();
    a.commit();
    b.get_movable_list("list").mov(2, 0).unwrap();
    b.commit();
    converge(&a, &b);
    for _ in 0..3 {
        assert!(u.undo().unwrap());
        assert_eq!(list(&a), json!([0, 2, 3]));
        assert!(u.undo().unwrap());
        assert_eq!(list(&a), json!([0, 1, 2, 3]));
        converge(&a, &b);
        assert!(u.redo().unwrap());
        assert_eq!(list(&a), json!([0, 2, 3]));
        assert!(u.redo().unwrap());
        assert_eq!(list(&a), json!([2, 3]));
        converge(&a, &b);
    }
}

#[test]
fn undo_removal_does_not_duplicate_element_rescued_by_concurrent_move() {
    let (a, b) = pair(&[0, 1, 2, 3]);
    let mut u = undo(&a);
    a.get_movable_list("list").delete(1, 1).unwrap();
    a.commit();
    b.get_movable_list("list").mov(1, 3).unwrap();
    b.get_movable_list("list").set(3, 99).unwrap();
    b.commit();
    converge(&a, &b);
    assert_eq!(list(&a), json!([0, 2, 3, 99]));
    assert!(!u.undo().unwrap());
    assert_eq!(list(&a), json!([0, 2, 3, 99]));
    converge(&a, &b);
}

#[test]
fn undo_move_distinguishes_equal_scalar_identities() {
    let (a, b) = pair(&[7, 7, 7]);
    let mut u = undo(&a);
    a.get_movable_list("list").mov(0, 2).unwrap();
    a.commit();
    b.get_movable_list("list").mov(0, 1).unwrap();
    b.commit();
    sync(&a, &b);
    assert!(u.undo().unwrap());
    b.get_movable_list("list").set(1, 99).unwrap();
    b.commit();
    converge(&a, &b);
    assert_eq!(list(&a), json!([99, 7, 7]));
}

#[test]
fn older_list_step_survives_parent_recreation() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let child = doc
        .get_map("root")
        .insert_container("list", loro::LoroMovableList::new())
        .unwrap();
    child.push(0).unwrap();
    doc.commit();
    let mut u = undo(&doc);
    child.push(1).unwrap();
    doc.commit();
    doc.get_map("root").delete("list").unwrap();
    doc.commit();
    for _ in 0..3 {
        assert!(u.undo().unwrap());
        assert_eq!(state(&doc), json!({"root":{"list":[0,1]}}));
        assert!(u.undo().unwrap());
        assert_eq!(state(&doc), json!({"root":{"list":[0]}}));
        assert!(u.redo().unwrap());
        assert_eq!(state(&doc), json!({"root":{"list":[0,1]}}));
        assert!(u.redo().unwrap());
        assert_eq!(state(&doc), json!({"root":{}}));
    }
}

#[test]
fn concurrent_move_undo_after_parent_recreation_keeps_identity() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let child = a
        .get_map("root")
        .insert_container("list", loro::LoroMovableList::new())
        .unwrap();
    for value in 0..4 {
        child.push(value).unwrap();
    }
    a.commit();
    let mut u = undo(&a);
    child.mov(0, 2).unwrap();
    a.commit();
    a.get_map("root").delete("list").unwrap();
    a.commit();
    assert!(u.undo().unwrap());
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    send(&a, &b);
    let remote = b
        .get_map("root")
        .get("list")
        .unwrap()
        .into_container()
        .unwrap()
        .into_movable_list()
        .unwrap();
    remote.mov(2, 1).unwrap();
    b.commit();
    sync(&a, &b);
    assert!(u.undo().unwrap());
    assert_eq!(state(&a), json!({"root":{"list":[0,1,2,3]}}));
    remote.set(1, 99).unwrap();
    b.commit();
    converge(&a, &b);
    assert_eq!(state(&a), json!({"root":{"list":[99,1,2,3]}}));
}

#[test]
fn undo_group_spanning_remote_changes_keeps_identity_and_anchors() {
    let (a, b) = pair(&[0, 1, 2, 3]);
    let mut u = undo(&a);
    u.group_start().unwrap();
    a.get_movable_list("list").delete(1, 1).unwrap();
    a.commit();
    b.get_map("remote").insert("unrelated", true).unwrap();
    b.commit();
    send(&b, &a);
    a.get_movable_list("list").mov(0, 2).unwrap();
    a.commit();
    u.group_end();
    assert_eq!(u.undo_count(), 1);
    assert!(u.undo().unwrap());
    assert_eq!(list(&a), json!([0, 1, 2, 3]));
    assert_eq!(
        a.get_map("remote")
            .get("unrelated")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!(true)
    );
    assert!(u.redo().unwrap());
    assert_eq!(list(&a), json!([2, 3, 0]));
    converge(&a, &b);
}

#[test]
fn undo_concurrent_child_move_preserves_child_identity_and_content() {
    let (a, b) = pair(&[0, 1, 2]);
    let child = a
        .get_movable_list("list")
        .insert_container(0, LoroText::new())
        .unwrap();
    child.insert(0, "content").unwrap();
    a.commit();
    converge(&a, &b);
    let mut u = undo(&a);
    a.get_movable_list("list").mov(0, 2).unwrap();
    a.commit();
    b.get_movable_list("list").mov(0, 1).unwrap();
    b.commit();
    converge(&a, &b);
    assert!(u.undo().unwrap());
    assert_eq!(
        a.get_movable_list("list")
            .get(0)
            .unwrap()
            .into_container()
            .unwrap()
            .id(),
        child.id()
    );
    assert_eq!(child.to_string(), "content");
    assert_eq!(state(&a), json!({"list":["content",0,1,2]}));
    converge(&a, &b);
}

#[test]
fn concurrent_move_pairs_preserve_all_elements_through_undo_redo() {
    for from_a in 0..4 {
        for to_a in 0..4 {
            if from_a == to_a {
                continue;
            }
            for from_b in 0..4 {
                for to_b in 0..4 {
                    if from_b == to_b {
                        continue;
                    }
                    let (a, b) = pair(&[0, 1, 2, 3]);
                    let mut u = undo(&a);
                    a.get_movable_list("list").mov(from_a, to_a).unwrap();
                    a.commit();
                    b.get_movable_list("list").mov(from_b, to_b).unwrap();
                    b.commit();
                    sync(&a, &b);
                    let merged = list(&a);
                    let changed = u.undo().unwrap();
                    let mut values = list(&a)
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_i64().unwrap())
                        .collect::<Vec<_>>();
                    values.sort();
                    assert_eq!(
                        values,
                        vec![0, 1, 2, 3],
                        "moves {from_a}->{to_a}, {from_b}->{to_b}"
                    );
                    if changed {
                        assert!(u.redo().unwrap());
                        assert_eq!(list(&a), merged);
                    }
                    // B has not received the undo/redo. Its sets still name
                    // the original identities, including the moved scalar.
                    for (index, value) in merged.as_array().unwrap().iter().enumerate() {
                        b.get_movable_list("list")
                            .set(index, value.as_i64().unwrap() + 10)
                            .unwrap();
                    }
                    b.commit();
                    converge(&a, &b);
                    let mut values = list(&a)
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_i64().unwrap())
                        .collect::<Vec<_>>();
                    values.sort();
                    assert_eq!(values, vec![10, 11, 12, 13]);
                }
            }
        }
    }
}

#[test]
fn undo_removal_after_concurrent_neighbour_move() {
    let (a, b) = pair(&[0, 1, 2, 3]);
    let mut u = undo(&a);
    a.get_movable_list("list").delete(1, 1).unwrap();
    a.commit();
    b.get_movable_list("list").mov(0, 1).unwrap();
    b.commit();
    sync(&a, &b);
    assert_eq!(list(&a), json!([0, 2, 3]));
    let result = u.undo();
    assert_eq!(result, Ok(true));
    println!(
        "CASE4_MINIMAL before=[0,2,3] undo={result:?} after={}",
        list(&a)
    );
    assert_eq!(list(&a), json!([0, 1, 2, 3]));
    sync(&a, &b);
    assert_eq!(state(&a), state(&b));
    for _ in 0..3 {
        assert!(u.redo().unwrap());
        assert_eq!(list(&a), json!([0, 2, 3]));
        converge(&a, &b);
        assert!(u.undo().unwrap());
        assert_eq!(list(&a), json!([0, 1, 2, 3]));
        converge(&a, &b);
    }
}
