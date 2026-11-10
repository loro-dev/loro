//! Stable element ids on `LoroMovableList`.
//!
//! An element's id is the `IdLp` of the op that inserted it. `mov` and `set`
//! do not change it. The methods read the elements visible in the current
//! state, including a checked-out version, a detached document, a state-only
//! snapshot, and a shallow document.

use loro::{
    cursor::Side, ExportMode, IdLp, JsonMovableListOp, JsonOpContent, LoroDoc, LoroMovableList,
    ToJson, VersionVector,
};

fn peer_doc(peer: u64) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer).unwrap();
    doc
}

fn lp(lamport: u32, peer: u64) -> IdLp {
    IdLp::new(peer, lamport)
}

fn strings(list: &LoroMovableList) -> Vec<String> {
    list.get_deep_value()
        .to_json_value()
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect()
}

fn sync(a: &LoroDoc, b: &LoroDoc) {
    let a_bytes = a.export(ExportMode::all_updates()).unwrap();
    let b_bytes = b.export(ExportMode::all_updates()).unwrap();
    a.import(&b_bytes).unwrap();
    b.import(&a_bytes).unwrap();
}

fn assert_ids_roundtrip(list: &LoroMovableList) {
    let ids = list.get_elem_ids();
    assert_eq!(ids.len(), list.len());
    for (index, id) in ids.iter().copied().enumerate() {
        assert_eq!(list.get_elem_id_at(index), Some(id));
        assert_eq!(list.get_index_of_elem_id(id), Some(index));
    }
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "visible element ids are unique");
    assert_eq!(list.get_elem_id_at(list.len()), None);
    assert_eq!(list.get_elem_id_at(usize::MAX), None);
}

fn movable_insert_ops(doc: &LoroDoc) -> usize {
    let json = doc
        .export_json_updates_without_peer_compression(&VersionVector::default(), &doc.oplog_vv())
        .unwrap();
    json.changes
        .iter()
        .flat_map(|change| change.ops.iter())
        .filter(|op| {
            matches!(
                op.content,
                JsonOpContent::MovableList(JsonMovableListOp::Insert { .. })
            )
        })
        .count()
}

#[test]
fn ids_follow_the_element_through_move_and_set_and_reject_out_of_range() {
    let doc = peer_doc(1);
    let list = doc.get_movable_list("mq");
    assert!(list.get_elem_ids().is_empty());
    assert_eq!(list.get_elem_id_at(0), None);

    for value in ["a", "b", "c"] {
        list.push(value).unwrap();
    }
    doc.commit();
    let created = vec![lp(0, 1), lp(1, 1), lp(2, 1)];
    assert_eq!(list.get_elem_ids(), created);
    assert_ids_roundtrip(&list);

    let moved = list.get_elem_id_at(0).unwrap();
    list.mov(0, 2).unwrap();
    doc.commit();
    list.set(2, "A").unwrap();
    doc.commit();

    assert_eq!(strings(&list), vec!["b", "c", "A"]);
    assert_eq!(list.get_elem_ids(), vec![lp(1, 1), lp(2, 1), moved]);
    assert_eq!(moved, lp(0, 1));
    assert_eq!(list.get_index_of_elem_id(moved), Some(2));
    assert_ids_roundtrip(&list);

    let cursor = list.get_cursor(2, Side::Middle).unwrap();
    let cursor_id = cursor.id.map(|id| id.to_string()).unwrap_or_default();
    assert_ne!(cursor_id, moved.to_string());

    let deleted = list.get_elem_id_at(0).unwrap();
    list.delete(0, 1).unwrap();
    doc.commit();
    assert_eq!(list.get_index_of_elem_id(deleted), None);
    assert!(!list.get_elem_ids().contains(&deleted));
    assert_eq!(
        list.get_index_of_elem_id(IdLp::new(1, u32::MAX)),
        None,
        "a lamport that cannot be a compact id is not an element"
    );
    assert_eq!(list.get_index_of_elem_id(lp(9, 4)), None);
}

#[test]
fn detached_container_has_no_element_ids() {
    let list = LoroMovableList::new();
    list.push("a").unwrap();
    list.push("b").unwrap();
    assert_eq!(strings(&list), vec!["a", "b"]);
    assert!(list.get_elem_ids().is_empty());
    assert_eq!(list.get_elem_id_at(0), None);
    assert_eq!(list.get_elem_id_at(1), None);
    assert_eq!(list.get_index_of_elem_id(lp(0, 1)), None);
}

#[test]
fn checkout_and_a_detached_document_show_the_visible_version() {
    let doc = peer_doc(1);
    let list = doc.get_movable_list("mq");
    list.push("a").unwrap();
    list.push("b").unwrap();
    doc.commit();
    let at_insert = doc.state_frontiers();
    let inserted = vec![lp(0, 1), lp(1, 1)];
    assert_eq!(list.get_elem_ids(), inserted);

    list.mov(0, 1).unwrap();
    doc.commit();
    let moved = vec![lp(1, 1), lp(0, 1)];
    assert_eq!(list.get_elem_ids(), moved);

    doc.checkout(&at_insert).unwrap();
    assert!(doc.is_detached());
    assert_eq!(list.get_elem_ids(), inserted);
    assert_eq!(strings(&list), vec!["a", "b"]);
    assert_ids_roundtrip(&list);

    doc.try_checkout_to_latest().unwrap();
    assert!(!doc.is_detached());
    assert_eq!(list.get_elem_ids(), moved);

    let other = peer_doc(2);
    other
        .import(&doc.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    doc.detach();
    assert!(doc.is_detached());
    other.get_movable_list("mq").push("c").unwrap();
    other.commit();
    let with_c = other.get_movable_list("mq").get_elem_ids();
    doc.import(&other.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(
        list.get_elem_ids(),
        moved,
        "an import into a detached document does not change the visible ids"
    );
    assert_eq!(strings(&list), vec!["b", "a"]);
    doc.try_attach().unwrap();
    assert_eq!(list.get_elem_ids(), with_c);
    assert_eq!(with_c.len(), 3);
    assert_eq!(strings(&list), vec!["b", "a", "c"]);
}

#[test]
fn state_only_snapshot_keeps_ids_of_the_loaded_version() {
    let doc = peer_doc(1);
    let list = doc.get_movable_list("mq");
    list.push("a").unwrap();
    list.push("b").unwrap();
    doc.commit();
    let at_insert = doc.state_frontiers();
    let inserted = vec![lp(0, 1), lp(1, 1)];
    list.mov(0, 1).unwrap();
    doc.commit();
    let moved = vec![lp(1, 1), lp(0, 1)];

    let latest =
        LoroDoc::from_snapshot(&doc.export(ExportMode::state_only(None)).unwrap()).unwrap();
    assert!(latest.is_shallow());
    assert_eq!(latest.get_movable_list("mq").get_elem_ids(), moved);
    assert_eq!(strings(&latest.get_movable_list("mq")), vec!["b", "a"]);
    assert_eq!(
        movable_insert_ops(&latest),
        0,
        "the trimmed oplog has no insert op; the ids still come from the state"
    );

    let old = LoroDoc::from_snapshot(
        &doc.export(ExportMode::state_only(Some(&at_insert)))
            .unwrap(),
    )
    .unwrap();
    assert!(old.is_shallow());
    assert_eq!(old.get_movable_list("mq").get_elem_ids(), inserted);
    assert_eq!(strings(&old.get_movable_list("mq")), vec!["a", "b"]);
}

#[test]
fn shallow_document_returns_ids_visible_at_the_checked_out_version() {
    let doc = peer_doc(1);
    let list = doc.get_movable_list("mq");
    list.push("a").unwrap();
    list.push("b").unwrap();
    doc.commit();
    let at_insert = doc.state_frontiers();
    let inserted = vec![lp(0, 1), lp(1, 1)];

    list.mov(0, 1).unwrap();
    doc.commit();
    let at_move = doc.state_frontiers();
    let moved = vec![lp(1, 1), lp(0, 1)];

    list.push("c").unwrap();
    doc.commit();
    let latest = list.get_elem_ids();
    assert_eq!(latest.len(), 3);
    assert_eq!(&latest[..2], &moved[..]);

    let bytes = doc
        .export(ExportMode::shallow_snapshot(&at_insert))
        .unwrap();
    let shallow = LoroDoc::new();
    shallow.import(&bytes).unwrap();
    assert!(shallow.is_shallow());
    let shallow_list = shallow.get_movable_list("mq");
    assert_eq!(shallow_list.get_elem_ids(), latest);
    assert_ids_roundtrip(&shallow_list);

    shallow.checkout(&at_insert).unwrap();
    assert!(shallow.is_detached());
    assert_eq!(shallow_list.get_elem_ids(), inserted);
    assert_eq!(strings(&shallow_list), vec!["a", "b"]);

    shallow.checkout(&at_move).unwrap();
    assert_eq!(shallow_list.get_elem_ids(), moved);
    assert_eq!(strings(&shallow_list), vec!["b", "a"]);

    shallow.try_checkout_to_latest().unwrap();
    assert_eq!(shallow_list.get_elem_ids(), latest);

    let trimmed = LoroDoc::new();
    trimmed
        .import(
            &doc.export(ExportMode::shallow_snapshot(&doc.oplog_frontiers()))
                .unwrap(),
        )
        .unwrap();
    assert!(trimmed.is_shallow());
    assert_eq!(trimmed.get_movable_list("mq").get_elem_ids(), latest);
}

#[test]
fn concurrent_moves_of_one_element_keep_a_single_id() {
    let a = peer_doc(1);
    let list = a.get_movable_list("mq");
    for value in ["a", "b", "c"] {
        list.push(value).unwrap();
    }
    a.commit();
    let b = peer_doc(2);
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    a.get_movable_list("mq").mov(0, 2).unwrap();
    a.commit();
    b.get_movable_list("mq").mov(0, 1).unwrap();
    b.commit();
    sync(&a, &b);

    let ids = vec![lp(1, 1), lp(0, 1), lp(2, 1)];
    assert_eq!(strings(&a.get_movable_list("mq")), vec!["b", "a", "c"]);
    assert_eq!(strings(&b.get_movable_list("mq")), vec!["b", "a", "c"]);
    assert_eq!(a.get_movable_list("mq").get_elem_ids(), ids);
    assert_eq!(b.get_movable_list("mq").get_elem_ids(), ids);
    assert_eq!(
        ids.iter().filter(|id| **id == lp(0, 1)).count(),
        1,
        "the moved element was not duplicated"
    );
    assert_ids_roundtrip(&a.get_movable_list("mq"));
    assert_ids_roundtrip(&b.get_movable_list("mq"));
}

#[test]
fn concurrent_rotations_of_equal_values_keep_the_created_ids() {
    let a = peer_doc(1);
    let list = a.get_movable_list("mq");
    list.push("x").unwrap();
    list.push("x").unwrap();
    list.push("x").unwrap();
    a.commit();
    let created = vec![lp(0, 1), lp(1, 1), lp(2, 1)];
    assert_eq!(list.get_elem_ids(), created);
    let b = peer_doc(2);
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    a.get_movable_list("mq").mov(0, 2).unwrap();
    a.commit();
    b.get_movable_list("mq").mov(2, 0).unwrap();
    b.commit();
    sync(&a, &b);

    let ids = vec![lp(2, 1), lp(1, 1), lp(0, 1)];
    assert_eq!(strings(&a.get_movable_list("mq")), vec!["x", "x", "x"]);
    assert_eq!(a.get_movable_list("mq").get_elem_ids(), ids);
    assert_eq!(b.get_movable_list("mq").get_elem_ids(), ids);
    let mut sorted = ids.clone();
    sorted.sort();
    let mut created_sorted = created.clone();
    created_sorted.sort();
    assert_eq!(
        sorted, created_sorted,
        "moves neither create nor drop an id"
    );
    assert_ids_roundtrip(&a.get_movable_list("mq"));
}

#[test]
fn concurrent_delete_and_move_does_not_duplicate_the_id() {
    let a = peer_doc(1);
    let list = a.get_movable_list("mq");
    for value in ["a", "b", "c"] {
        list.push(value).unwrap();
    }
    a.commit();
    let created = list.get_elem_ids();
    let b = peer_doc(2);
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    a.get_movable_list("mq").delete(0, 1).unwrap();
    a.commit();
    b.get_movable_list("mq").mov(0, 2).unwrap();
    b.commit();
    sync(&a, &b);

    let ids = vec![lp(1, 1), lp(2, 1), lp(0, 1)];
    assert_eq!(strings(&a.get_movable_list("mq")), vec!["b", "c", "a"]);
    assert_eq!(
        strings(&b.get_movable_list("mq")),
        strings(&a.get_movable_list("mq"))
    );
    assert_eq!(a.get_movable_list("mq").get_elem_ids(), ids);
    assert_eq!(b.get_movable_list("mq").get_elem_ids(), ids);
    assert_eq!(ids.iter().filter(|id| **id == created[0]).count(), 1);
    assert_eq!(
        a.get_movable_list("mq").get_index_of_elem_id(created[0]),
        Some(2)
    );
    assert_ids_roundtrip(&a.get_movable_list("mq"));
}
