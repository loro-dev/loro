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
fn undo_recreated_element_after_original_is_remotely_revived() {
    let (a, b) = pair(&[-1, 0, 1]);
    let mut u = undo(&a);
    a.get_movable_list("list").delete(1, 1).unwrap();
    a.commit();
    assert!(u.undo().unwrap());
    // This concurrent move revives the original identity, while undo has
    // already created a replacement and saved an alias to it.
    b.get_movable_list("list").mov(1, 0).unwrap();
    b.commit();
    converge(&a, &b);
    assert_eq!(list(&a), json!([0, -1, 0, 1]));
    a.get_movable_list("list").delete(2, 1).unwrap();
    a.commit();
    assert!(u.undo().unwrap());
    assert_eq!(list(&a), json!([0, -1, 0, 1]));
    converge(&a, &b);
}

#[test]
fn revived_original_and_recreated_map_keep_distinct_contents() {
    let (a, b) = pair(&[-1, 1]);
    let child = a
        .get_movable_list("list")
        .insert_container(1, loro::LoroMap::new())
        .unwrap();
    child.insert("value", 7).unwrap();
    a.commit();
    converge(&a, &b);
    let mut u = undo(&a);
    a.get_movable_list("list").delete(1, 1).unwrap();
    a.commit();
    assert!(u.undo().unwrap());
    b.get_movable_list("list").mov(1, 0).unwrap();
    b.commit();
    converge(&a, &b);
    let before = state(&a);
    a.get_movable_list("list").delete(2, 1).unwrap();
    a.commit();
    assert!(u.undo().unwrap());
    assert_eq!(state(&a), before);
    assert_eq!(
        a.get_movable_list("list")
            .get(0)
            .unwrap()
            .into_container()
            .unwrap()
            .id(),
        child.id()
    );
    b.get_movable_list("list")
        .get(0)
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap()
        .insert("remote", true)
        .unwrap();
    b.commit();
    converge(&a, &b);
    assert_eq!(
        state(&a),
        json!({"list":[{"value":7,"remote":true}, -1, {"value":7}, 1]})
    );
}

#[test]
fn removal_searches_past_deleted_neighbours_without_losing_a_moved_anchor() {
    let (a, b) = pair(&(0..64).collect::<Vec<_>>());
    let mut u = undo(&a);
    a.get_movable_list("list").delete(32, 1).unwrap();
    a.commit();
    let remote = b.get_movable_list("list");
    remote.delete(17, 31).unwrap();
    remote.mov(16, remote.len() - 1).unwrap();
    remote.insert(0, 999).unwrap();
    b.commit();
    converge(&a, &b);
    let before = list(&a);
    let mut restored = before.as_array().unwrap().clone();
    restored.push(json!(32));
    for _ in 0..3 {
        assert!(u.undo().unwrap());
        assert_eq!(list(&a), Value::Array(restored.clone()));
        converge(&a, &b);
        assert!(u.redo().unwrap());
        assert_eq!(list(&a), before);
        converge(&a, &b);
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

#[test]
fn undo_deleting_revived_original_element_restores_it() {
    // Regression for loro-dev/loro#1234 (case a):
    // Deleting an element, undoing it (creating a replacement and recording
    // original -> replacement), importing a concurrent remote move that revives
    // the original, and then deleting the revived original must allow undo to
    // restore it rather than returning false.
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();

    let list_a = a.get_movable_list("list");
    list_a.insert(0, "a").unwrap();
    list_a.insert(1, "b").unwrap();
    a.commit();
    b.import(&a.export(ExportMode::Snapshot).unwrap()).unwrap();

    let mut u = UndoManager::new(&a);
    u.set_merge_interval(0);

    list_a.delete(0, 1).unwrap();
    a.commit();

    b.get_movable_list("list").mov(0, 1).unwrap();
    b.commit();

    assert!(u.undo().unwrap());
    assert_eq!(list(&a), json!(["a", "b"]));

    a.import(&b.export(ExportMode::Snapshot).unwrap()).unwrap();
    assert_eq!(list(&a), json!(["a", "b", "a"]));

    // Delete the revived original at index 2, leaving the replacement at index 0.
    list_a.delete(2, 1).unwrap();
    a.commit();
    assert_eq!(list(&a), json!(["a", "b"]));
    assert!(u.can_undo());

    assert!(u.undo().unwrap());
    assert_eq!(list(&a), json!(["a", "b", "a"]));

    for _ in 0..2 {
        assert!(u.redo().unwrap());
        assert_eq!(list(&a), json!(["a", "b"]));
        assert!(u.undo().unwrap());
        assert_eq!(list(&a), json!(["a", "b", "a"]));
    }
    converge(&a, &b);
}

#[test]
fn undo_then_redo_move_anchors_to_revived_original_not_copy() {
    // Regression for loro-dev/loro#1234 (case b):
    // After a remote move revives the original "b" alongside its undo copy,
    // undoing and redoing an earlier move of "a" must anchor to the revived
    // original "b" rather than its copy, restoring ["b", "b", "a"].
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();

    let list_a = a.get_movable_list("list");
    list_a.insert(0, "a").unwrap();
    list_a.insert(1, "b").unwrap();
    a.commit();
    b.import(&a.export(ExportMode::Snapshot).unwrap()).unwrap();

    let mut u = UndoManager::new(&a);
    u.set_merge_interval(0);

    list_a.mov(0, 1).unwrap(); // ["b", "a"]
    a.commit();

    list_a.delete(0, 1).unwrap(); // ["a"]
    a.commit();
    assert!(u.undo().unwrap()); // ["b'", "a"]

    b.get_movable_list("list").mov(1, 0).unwrap();
    b.commit();
    a.import(&b.export(ExportMode::Snapshot).unwrap()).unwrap();

    let before = list(&a);
    assert_eq!(before, json!(["b", "b", "a"]));

    assert!(u.undo().unwrap());
    assert!(u.redo().unwrap());
    assert_eq!(list(&a), before);
    converge(&a, &b);
}

#[test]
fn undo_editing_revived_original_child_map_targets_original_not_copy() {
    let (a, b) = pair(&[-1, 1]);
    let child = a
        .get_movable_list("list")
        .insert_container(1, loro::LoroMap::new())
        .unwrap();
    child.insert("value", 7).unwrap();
    a.commit();
    converge(&a, &b);

    let mut u = undo(&a);
    a.get_movable_list("list").delete(1, 1).unwrap();
    a.commit();
    assert!(u.undo().unwrap());

    b.get_movable_list("list").mov(1, 0).unwrap();
    b.commit();
    converge(&a, &b);

    // Edit the revived original map (at index 0) and undo; undo must revert
    // the original map rather than touching the replacement map (at index 2).
    let revived_map = a
        .get_movable_list("list")
        .get(0)
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap();
    assert_eq!(revived_map.id(), child.id());
    revived_map.insert("value", 99).unwrap();
    a.commit();
    assert_eq!(
        state(&a),
        json!({"list":[{"value":99}, -1, {"value":7}, 1]})
    );

    assert!(u.undo().unwrap());
    assert_eq!(state(&a), json!({"list":[{"value":7}, -1, {"value":7}, 1]}));
    converge(&a, &b);
}

#[test]
fn undo_editing_revived_nested_map_targets_original_and_keeps_copy() {
    // MovableList -> Map -> Map:
    // After a remote move revives the original element, collaborator edits the
    // COPY's nested map to 42 while locally the ORIGINAL's nested map changes
    // 7 -> 99. Undo must restore the original to 7 and leave the copy at 42.
    let (a, b) = pair(&[-1, 1]);
    let outer = a
        .get_movable_list("list")
        .insert_container(1, loro::LoroMap::new())
        .unwrap();
    let inner = outer
        .insert_container("nested", loro::LoroMap::new())
        .unwrap();
    inner.insert("v", 7).unwrap();
    a.commit();
    converge(&a, &b);

    let mut u = undo(&a);
    a.get_movable_list("list").delete(1, 1).unwrap();
    a.commit();
    assert!(u.undo().unwrap());

    b.get_movable_list("list").mov(1, 0).unwrap();
    b.commit();
    converge(&a, &b);

    // Index 0 is the revived original; index 2 is the undo-created copy.
    let copy_outer_b = b
        .get_movable_list("list")
        .get(2)
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap();
    let copy_inner_b = copy_outer_b
        .get("nested")
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap();
    assert_ne!(copy_inner_b.id(), inner.id());
    copy_inner_b.insert("v", 42).unwrap();
    b.commit();
    converge(&a, &b);

    let orig_outer_a = a
        .get_movable_list("list")
        .get(0)
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap();
    let orig_inner_a = orig_outer_a
        .get("nested")
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap();
    assert_eq!(orig_inner_a.id(), inner.id());
    orig_inner_a.insert("v", 99).unwrap();
    a.commit();

    assert_eq!(
        state(&a),
        json!({"list":[{"nested":{"v":99}}, -1, {"nested":{"v":42}}, 1]})
    );

    assert!(u.undo().unwrap());
    assert_eq!(
        state(&a),
        json!({"list":[{"nested":{"v":7}}, -1, {"nested":{"v":42}}, 1]})
    );
    converge(&a, &b);
}

#[test]
fn undo_editing_revived_nested_containers_prunes_whole_subtree() {
    // Verify subtree pruning across List, Tree, and nested MovableList inside a
    // revived MovableList element.
    let (a, b) = pair(&[0]);
    let outer = a
        .get_movable_list("list")
        .insert_container(1, loro::LoroMap::new())
        .unwrap();
    let nested_list = outer
        .insert_container("items", loro::LoroList::new())
        .unwrap();
    let list_map = nested_list
        .insert_container(0, loro::LoroMap::new())
        .unwrap();
    list_map.insert("x", 1).unwrap();

    let nested_tree = outer
        .insert_container("tree", loro::LoroTree::new())
        .unwrap();
    let tree_node = nested_tree.create(None).unwrap();
    nested_tree
        .get_meta(tree_node)
        .unwrap()
        .insert("t", "orig")
        .unwrap();

    let nested_mlist = outer
        .insert_container("mlist", loro::LoroMovableList::new())
        .unwrap();
    nested_mlist.push("m0").unwrap();
    a.commit();
    converge(&a, &b);

    let mut u = undo(&a);
    a.get_movable_list("list").delete(1, 1).unwrap();
    a.commit();
    assert!(u.undo().unwrap());

    b.get_movable_list("list").mov(1, 0).unwrap();
    b.commit();
    converge(&a, &b);

    // Collaborator edits the copy's nested containers at index 2.
    let copy_outer_b = b
        .get_movable_list("list")
        .get(2)
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap();
    copy_outer_b
        .get("items")
        .unwrap()
        .into_container()
        .unwrap()
        .into_list()
        .unwrap()
        .get(0)
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap()
        .insert("x", 100)
        .unwrap();
    let copy_tree_b = copy_outer_b
        .get("tree")
        .unwrap()
        .into_container()
        .unwrap()
        .into_tree()
        .unwrap();
    let copy_node = copy_tree_b.roots()[0];
    copy_tree_b
        .get_meta(copy_node)
        .unwrap()
        .insert("t", "copy")
        .unwrap();
    b.commit();
    converge(&a, &b);

    // Locally edit the revived original's nested List->Map, Tree meta, and
    // nested MovableList element, then undo each.
    list_map.insert("x", 2).unwrap();
    a.commit();
    assert!(u.undo().unwrap());
    assert_eq!(list_map.get("x").unwrap().into_value().unwrap(), 1.into());

    nested_tree
        .get_meta(tree_node)
        .unwrap()
        .insert("t", "edited")
        .unwrap();
    a.commit();
    assert!(u.undo().unwrap());
    assert_eq!(
        nested_tree
            .get_meta(tree_node)
            .unwrap()
            .get("t")
            .unwrap()
            .into_value()
            .unwrap(),
        "orig".into()
    );

    nested_mlist.delete(0, 1).unwrap();
    a.commit();
    assert!(u.undo().unwrap());
    assert_eq!(nested_mlist.get_value().to_json_value(), json!(["m0"]));

    // Copy's nested containers remain untouched by all three undos.
    let copy_outer_a = a
        .get_movable_list("list")
        .get(2)
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap();
    assert_eq!(
        copy_outer_a
            .get("items")
            .unwrap()
            .into_container()
            .unwrap()
            .into_list()
            .unwrap()
            .get(0)
            .unwrap()
            .into_container()
            .unwrap()
            .into_map()
            .unwrap()
            .get("x")
            .unwrap()
            .into_value()
            .unwrap(),
        100.into()
    );
    let copy_tree_a = copy_outer_a
        .get("tree")
        .unwrap()
        .into_container()
        .unwrap()
        .into_tree()
        .unwrap();
    assert_eq!(
        copy_tree_a
            .get_meta(copy_tree_a.roots()[0])
            .unwrap()
            .get("t")
            .unwrap()
            .into_value()
            .unwrap(),
        "copy".into()
    );
    converge(&a, &b);
}

#[test]
fn unpaused_checkout_clears_element_and_container_remaps() {
    let (a, b) = pair(&[20]);
    let child = a
        .get_movable_list("list")
        .insert_container(0, loro::LoroMap::new())
        .unwrap();
    child.insert("v", 7).unwrap();
    a.commit();
    converge(&a, &b);
    let initial = a.oplog_frontiers();
    let mut u = undo(&a);

    // Populate both element_remap and container_remap.
    a.get_movable_list("list").delete(0, 1).unwrap();
    a.commit();
    assert!(u.undo().unwrap());

    // Checkout to an earlier version (unpaused): clears stacks and both remaps.
    a.checkout(&initial).unwrap();
    assert!(!u.can_undo());
    assert!(!u.can_redo());

    // While detached, import a remote move that revives the original element
    // and its child map.
    b.get_movable_list("list").mov(0, 1).unwrap();
    b.commit();
    send(&b, &a);

    // Re-attach to latest: emits Checkout (unpaused), ensuring both remaps are
    // clear even though Import did not fire while detached.
    a.try_checkout_to_latest().unwrap();
    assert_eq!(state(&a), json!({"list":[{"v":7}, 20, {"v":7}]}));

    // Edit the revived original child map at index 2 and undo; must target the
    // original map rather than the copy at index 0.
    child.insert("v", 99).unwrap();
    a.commit();
    assert_eq!(state(&a), json!({"list":[{"v":7}, 20, {"v":99}]}));
    assert!(u.undo().unwrap());
    assert_eq!(state(&a), json!({"list":[{"v":7}, 20, {"v":7}]}));

    // Delete the revived original element at index 2 and undo; must succeed.
    a.get_movable_list("list").delete(2, 1).unwrap();
    a.commit();
    assert_eq!(state(&a), json!({"list":[{"v":7}, 20]}));
    assert!(u.undo().unwrap());
    assert_eq!(state(&a), json!({"list":[{"v":7}, 20, {"v":7}]}));
    converge(&a, &b);
}

#[test]
fn paused_checkout_preserves_element_and_container_remaps() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let list_a = a.get_movable_list("list");
    let map = list_a.insert_container(0, loro::LoroMap::new()).unwrap();
    map.insert("k", 1).unwrap();
    list_a.push(10).unwrap();
    a.commit();
    let initial = a.oplog_frontiers();

    let mut u = undo(&a);
    // Step 1: edit child map (references original map cid).
    map.insert("k", 2).unwrap();
    a.commit();
    // Step 2: move scalar element 10 to index 0 (references original elem id).
    list_a.mov(1, 0).unwrap();
    a.commit();
    // Step 3: delete both elements, then undo to populate element_remap and container_remap.
    list_a.delete(0, 2).unwrap();
    a.commit();
    assert!(u.undo().unwrap());
    assert_eq!(state(&a), json!({"list":[10, {"k":2}]}));

    // Pause undo manager and perform a checkout round-trip; both stacks and both
    // remap tables must survive.
    u.pause();
    a.checkout(&initial).unwrap();
    a.try_checkout_to_latest().unwrap();
    u.resume();

    // Undo Step 2 (move of 10): requires element_remap to find recreated 10'.
    assert!(u.undo().unwrap());
    assert_eq!(state(&a), json!({"list":[{"k":2}, 10]}));

    // Undo Step 1 (map edit k: 1 -> 2): requires container_remap to find recreated map'.
    assert!(u.undo().unwrap());
    assert_eq!(state(&a), json!({"list":[{"k":1}, 10]}));
}

#[test]
fn undo_editing_revived_mergeable_child_map_targets_original_and_keeps_copy() {
    // MovableList -> Map -> ensure_mergeable_map:
    // Mergeable children are stored as binary markers in the parent Map.
    // After a remote move revives the original element, collaborator edits the
    // COPY's mergeable child map to 42 while locally the ORIGINAL's mergeable
    // child map changes 7 -> 99. Undo must restore the original to 7 and leave
    // the copy at 42.
    let (a, b) = pair(&[-1, 1]);
    let outer = a
        .get_movable_list("list")
        .insert_container(1, loro::LoroMap::new())
        .unwrap();
    let inner = outer.ensure_mergeable_map("nested").unwrap();
    inner.insert("v", 7).unwrap();
    a.commit();
    converge(&a, &b);

    let mut u = undo(&a);
    a.get_movable_list("list").delete(1, 1).unwrap();
    a.commit();
    assert!(u.undo().unwrap());

    b.get_movable_list("list").mov(1, 0).unwrap();
    b.commit();
    converge(&a, &b);

    // Index 0 is the revived original; index 2 is the undo-created copy.
    let copy_outer_b = b
        .get_movable_list("list")
        .get(2)
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap();
    let copy_inner_b = copy_outer_b.ensure_mergeable_map("nested").unwrap();
    assert_ne!(copy_inner_b.id(), inner.id());
    copy_inner_b.insert("v", 42).unwrap();
    b.commit();
    converge(&a, &b);

    let orig_outer_a = a
        .get_movable_list("list")
        .get(0)
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap();
    let orig_inner_a = orig_outer_a.ensure_mergeable_map("nested").unwrap();
    assert_eq!(orig_inner_a.id(), inner.id());
    orig_inner_a.insert("v", 99).unwrap();
    a.commit();

    assert_eq!(
        state(&a),
        json!({"list":[{"nested":{"v":99}}, -1, {"nested":{"v":42}}, 1]})
    );

    assert!(u.undo().unwrap());
    assert_eq!(
        state(&a),
        json!({"list":[{"nested":{"v":7}}, -1, {"nested":{"v":42}}, 1]})
    );
    converge(&a, &b);
}

#[test]
fn paused_checkout_reconciles_revived_nested_map_on_return_to_latest() {
    // Pause undo, checkout history, import a remote move that revives an
    // original element (plus a collaborator edit to the copy's nested map),
    // checkout back to latest, and resume. Returning to latest must prune
    // revived identities while keeping still-dead remaps for older undo items.
    let (a, b) = pair(&[-1, 1]);
    let outer = a
        .get_movable_list("list")
        .insert_container(1, loro::LoroMap::new())
        .unwrap();
    let inner = outer
        .insert_container("nested", loro::LoroMap::new())
        .unwrap();
    inner.insert("v", 7).unwrap();
    a.commit();
    converge(&a, &b);
    let c = LoroDoc::new();
    c.set_peer_id(3).unwrap();
    send(&a, &c);
    let history = a.oplog_frontiers();

    let mut u = undo(&a);
    // Earlier undo step on element `-1` (at index 0): move it to the end.
    a.get_movable_list("list").mov(0, 2).unwrap();
    a.commit();
    // Delete both the outer map (now at index 0) and `-1` (now at index 2),
    // then undo to populate remaps for both.
    a.get_movable_list("list").delete(0, 1).unwrap();
    a.commit();
    assert!(u.undo().unwrap());
    a.get_movable_list("list").delete(2, 1).unwrap();
    a.commit();
    assert!(u.undo().unwrap());
    // Sync the undo-created copies to B so B can edit the copy's nested map.
    send(&a, &b);

    // Pause undo and detach to historical frontiers.
    u.pause();
    a.checkout(&history).unwrap();

    // While A is detached, C (which hasn't seen A's delete) moves the original
    // outer map (reviving it), and B edits the copy's nested map to 42.
    c.get_movable_list("list").mov(1, 0).unwrap();
    c.commit();

    // Edit the copy's nested map on B to 42.
    let copy_outer_b = b
        .get_movable_list("list")
        .get(0)
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap();
    let copy_inner_b = copy_outer_b
        .get("nested")
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap();
    assert_ne!(copy_inner_b.id(), inner.id());
    copy_inner_b.insert("v", 42).unwrap();
    b.commit();

    // Import both B's edit to the copy and C's reviving move while A is detached.
    send(&b, &a);
    send(&c, &a);

    // Return to latest while paused: must prune the revived outer/inner map
    // remaps while keeping the still-dead `-1` element remap intact.
    a.try_checkout_to_latest().unwrap();
    u.resume();

    // Locate the revived original nested map (same cid as `inner`) and edit it 7 -> 99.
    inner.insert("v", 99).unwrap();
    a.commit();

    assert!(u.undo().unwrap());
    // Original nested map reverted to 7; copy's nested map remains 42.
    assert_eq!(inner.get("v").unwrap().into_value().unwrap(), 7.into());
    let copy_inner_a = (0..a.get_movable_list("list").len())
        .filter_map(|i| {
            a.get_movable_list("list")
                .get(i)?
                .into_container()
                .ok()?
                .into_map()
                .ok()?
                .get("nested")?
                .into_container()
                .ok()?
                .into_map()
                .ok()
        })
        .find(|m| m.id() == copy_inner_b.id())
        .expect("copy nested map should still exist");
    assert_eq!(
        copy_inner_a.get("v").unwrap().into_value().unwrap(),
        42.into()
    );

    // Older undo step (moving `-1` from before `outer` to the end) still works
    // via its preserved element remap and anchors `-1` immediately before the
    // revived original `outer`.
    assert!(u.undo().unwrap());
    assert_eq!(
        state(&a),
        json!({"list":[{"nested":{"v":42}}, -1, {"nested":{"v":7}}, 1]})
    );
    converge(&a, &b);
    converge(&a, &c);
}
