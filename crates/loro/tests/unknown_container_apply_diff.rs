//! Diffs and container inserts that would make this version create a container
//! of an unknown type (e.g. `cid:0@1:Unknown(9)`) must return `Err` without
//! modifying the doc, while docs that already hold unknown containers (written
//! by a newer Loro) keep working.

use loro::{
    event::{Diff, DiffBatch, ListDiffItem, MapDelta},
    ContainerID, ContainerTrait, ContainerType, ExportMode, LoroDoc, LoroError, LoroList, LoroMap,
    LoroValue, ToJson, ValueOrContainer, ID,
};
use rustc_hash::FxHashMap;
use std::borrow::Cow;

const UNKNOWN: ContainerType = ContainerType::Unknown(9);

fn unknown_ids() -> Vec<ContainerID> {
    vec![
        ContainerID::try_from("cid:0@1:Unknown(9)").unwrap(),
        ContainerID::new_normal(ID::new(1, 0), UNKNOWN),
        ContainerID::new_root("future", UNKNOWN),
    ]
}

fn list_insert(v: ValueOrContainer) -> Diff<'static> {
    Diff::List(vec![ListDiffItem::Insert {
        insert: vec![v],
        is_move: false,
    }])
}

fn map_set(key: &str, v: ValueOrContainer) -> Diff<'static> {
    Diff::Map(MapDelta {
        updated: FxHashMap::from_iter([(Cow::Owned(key.to_string()), Some(v))]),
    })
}

fn unknown_value(id: &ContainerID) -> ValueOrContainer {
    ValueOrContainer::Value(LoroValue::Container(id.clone()))
}

/// A doc whose root map `map` holds an unknown container under `k`, plus the
/// unknown container's id. The unknown type is forged by rewriting the JSON
/// updates of a Text container, as if a newer Loro had written it.
fn doc_with_unknown_in(setup: impl Fn(&LoroDoc) -> ContainerID) -> (LoroDoc, ContainerID) {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let text_id = setup(&doc);
    doc.commit();
    let json = serde_json::to_string(
        &doc.export_json_updates_without_peer_compression(&Default::default(), &doc.oplog_vv()),
    )
    .unwrap();
    let text_cid = text_id.to_string();
    let unknown_cid = text_cid.replace(":Text", ":Unknown(9)");
    assert!(json.contains(&text_cid));
    let patched = json.replace(&text_cid, &unknown_cid);
    let doc2 = LoroDoc::new();
    doc2.set_peer_id(2).unwrap();
    doc2.import_json_updates(patched.as_str()).unwrap();
    (doc2, ContainerID::try_from(unknown_cid.as_str()).unwrap())
}

/// Applies `batch` and checks it is rejected without touching the doc. A text
/// edit is always put first so partial application would be visible.
fn assert_rejected(doc: &LoroDoc, mut batch: DiffBatch) {
    let mut first = DiffBatch::default();
    first
        .push(
            ContainerID::new_root("witness", ContainerType::Text),
            Diff::Text(vec![loro::TextDelta::Insert {
                insert: "partial".into(),
                attributes: None,
            }]),
        )
        .unwrap();
    for (id, diff) in batch.iter() {
        first.push(id.clone(), diff.clone()).unwrap();
    }
    batch = first;

    doc.commit();
    let before = doc.get_deep_value();
    let vv = doc.oplog_vv();
    let err = doc.apply_diff(batch).unwrap_err();
    assert!(
        matches!(&err, LoroError::ArgErr(msg) if msg.contains("Unknown(9)")),
        "{err:?}"
    );
    doc.commit();
    assert_eq!(doc.get_deep_value(), before);
    assert_eq!(doc.oplog_vv(), vv);
}

#[test]
fn apply_diff_rejects_inserting_unknown_container_into_list() {
    for id in unknown_ids() {
        let doc = LoroDoc::new();
        doc.get_list("list").push(1).unwrap();
        let mut batch = DiffBatch::default();
        batch
            .push(
                ContainerID::new_root("list", ContainerType::List),
                list_insert(unknown_value(&id)),
            )
            .unwrap();
        assert_rejected(&doc, batch);
    }
}

#[test]
fn apply_diff_rejects_inserting_unknown_container_into_movable_list() {
    for id in unknown_ids() {
        let doc = LoroDoc::new();
        doc.get_movable_list("list").push(1).unwrap();
        let mut batch = DiffBatch::default();
        batch
            .push(
                ContainerID::new_root("list", ContainerType::MovableList),
                list_insert(unknown_value(&id)),
            )
            .unwrap();
        assert_rejected(&doc, batch);

        // A move of an element that is not in the list creates it
        let mut batch = DiffBatch::default();
        batch
            .push(
                ContainerID::new_root("list", ContainerType::MovableList),
                Diff::List(vec![ListDiffItem::Insert {
                    insert: vec![unknown_value(&id)],
                    is_move: true,
                }]),
            )
            .unwrap();
        assert_rejected(&doc, batch);
    }
}

#[test]
fn apply_diff_rejects_setting_unknown_container_in_map() {
    for id in unknown_ids() {
        let doc = LoroDoc::new();
        doc.get_map("map").insert("a", 1).unwrap();
        let mut batch = DiffBatch::default();
        batch
            .push(
                ContainerID::new_root("map", ContainerType::Map),
                map_set("k", unknown_value(&id)),
            )
            .unwrap();
        assert_rejected(&doc, batch);
    }
}

#[test]
fn apply_diff_rejects_unknown_container_in_tree_meta() {
    for id in unknown_ids() {
        let doc = LoroDoc::new();
        let tree = doc.get_tree("tree");
        let node = tree.create(None).unwrap();
        tree.get_meta(node).unwrap().insert("a", 1).unwrap();
        let mut batch = DiffBatch::default();
        batch
            .push(
                node.associated_meta_container(),
                map_set("k", unknown_value(&id)),
            )
            .unwrap();
        assert_rejected(&doc, batch);
    }
}

#[test]
fn apply_diff_rejects_unknown_container_in_nested_new_containers() {
    for id in unknown_ids() {
        let doc = LoroDoc::new();
        doc.get_map("map").insert("a", 1).unwrap();
        // `map.k` becomes a new list, and the new list gets an unknown child
        let new_list = ContainerID::new_normal(ID::new(100, 0), ContainerType::List);
        let new_mlist = ContainerID::new_normal(ID::new(100, 1), ContainerType::MovableList);
        let mut batch = DiffBatch::default();
        batch
            .push(
                ContainerID::new_root("map", ContainerType::Map),
                Diff::Map(MapDelta {
                    updated: FxHashMap::from_iter([
                        (Cow::Borrowed("k"), Some(unknown_value(&new_list))),
                        (Cow::Borrowed("m"), Some(unknown_value(&new_mlist))),
                    ]),
                }),
            )
            .unwrap();
        batch
            .push(
                new_list.clone(),
                list_insert(ValueOrContainer::Value(1.into())),
            )
            .unwrap();
        batch
            .push(new_mlist, list_insert(unknown_value(&id)))
            .unwrap();
        assert_rejected(&doc, batch);
    }
}

#[test]
fn insert_container_rejects_copying_unknown_container() {
    let (doc, unknown) = doc_with_unknown_in(|doc| {
        doc.get_map("map")
            .insert_container("k", loro::LoroText::new())
            .unwrap()
            .id()
    });
    let map = doc.get_map("map");
    let loro::ValueOrContainer::Container(loro::Container::Unknown(u)) = map.get("k").unwrap()
    else {
        panic!("expected an unknown container");
    };
    assert_eq!(u.id(), unknown);

    let list = doc.get_list("list");
    let mlist = doc.get_movable_list("mlist");
    let other = doc.get_map("other");
    mlist.insert(0, 1).unwrap();
    doc.commit();
    let before = doc.get_deep_value();
    let vv = doc.oplog_vv();
    let expect_err = |r: Result<(), LoroError>| {
        let err = r.unwrap_err();
        assert!(
            matches!(&err, LoroError::ArgErr(msg) if msg.contains("Unknown(9)")),
            "{err:?}"
        );
    };
    expect_err(map.insert_container("k2", u.clone()).map(|_| ()));
    expect_err(list.insert_container(0, u.clone()).map(|_| ()));
    expect_err(mlist.insert_container(0, u.clone()).map(|_| ()));
    expect_err(mlist.set_container(0, u.clone()).map(|_| ()));

    // Nested in a detached container
    let detached = LoroMap::new();
    detached.insert("x", 1).unwrap();
    detached.insert_container("u", u.clone()).unwrap();
    expect_err(list.push_container(detached).map(|_| ()));

    // Copying an attached container that holds an unknown child
    expect_err(list.push_container(map.clone()).map(|_| ()));
    let detached_list = LoroList::new();
    detached_list.push_container(map.clone()).unwrap();
    expect_err(other.insert_container("l", detached_list).map(|_| ()));

    doc.commit();
    assert_eq!(doc.get_deep_value(), before);
    assert_eq!(doc.oplog_vv(), vv);
}

/// Docs holding unknown containers keep importing, exporting, checking out,
/// moving and reverting as before; only creating an unknown container fails.
#[test]
fn existing_unknown_containers_keep_working() {
    let (doc, unknown) = doc_with_unknown_in(|doc| {
        let list = doc.get_movable_list("mlist");
        list.push("a").unwrap();
        let id = list
            .insert_container(0, loro::LoroText::new())
            .unwrap()
            .id();
        list.push("b").unwrap();
        id
    });
    let mlist = doc.get_movable_list("mlist");
    let json = |doc: &LoroDoc| doc.get_deep_value().to_json_value();
    assert_eq!(json(&doc), serde_json::json!({"mlist": [null, "a", "b"]}));

    // Export / import / checkout
    let v0 = doc.state_frontiers();
    for mode in [ExportMode::snapshot(), ExportMode::all_updates()] {
        let bytes = doc.export(mode).unwrap();
        let doc2 = LoroDoc::new();
        doc2.import(&bytes).unwrap();
        assert_eq!(json(&doc2), json(&doc));
        assert_eq!(
            doc2.get_movable_list("mlist")
                .get(0)
                .unwrap()
                .as_container()
                .unwrap()
                .id(),
            unknown
        );
    }

    // Moving an unknown element, then reverting and undoing the move, only
    // moves it and never creates a new container
    let mut undo = loro::UndoManager::new(&doc);
    mlist.mov(0, 2).unwrap();
    doc.commit();
    assert_eq!(json(&doc), serde_json::json!({"mlist": ["a", "b", null]}));
    let v1 = doc.state_frontiers();
    doc.checkout(&v0).unwrap();
    assert_eq!(json(&doc), serde_json::json!({"mlist": [null, "a", "b"]}));
    doc.checkout_to_latest();

    doc.revert_to(&v0).unwrap();
    doc.commit();
    assert_eq!(json(&doc), serde_json::json!({"mlist": [null, "a", "b"]}));
    doc.revert_to(&v1).unwrap();
    doc.commit();
    assert_eq!(json(&doc), serde_json::json!({"mlist": ["a", "b", null]}));
    assert!(undo.undo().unwrap());
    assert_eq!(json(&doc), serde_json::json!({"mlist": [null, "a", "b"]}));
    let diff = doc.diff(&v0, &v1).unwrap();
    doc.apply_diff(diff).unwrap();
    assert_eq!(json(&doc), serde_json::json!({"mlist": ["a", "b", null]}));
    let only_unknown = |doc: &LoroDoc| {
        let list = doc.get_movable_list("mlist");
        (0..list.len())
            .filter_map(|i| list.get(i).unwrap().into_container().ok())
            .map(|c| c.id())
            .collect::<Vec<_>>()
    };
    assert_eq!(only_unknown(&doc), vec![unknown.clone()]);

    // Reverting to before the unknown element was deleted would recreate it:
    // rejected before any change.
    let v2 = doc.state_frontiers();
    mlist.delete(2, 1).unwrap();
    mlist.push("c").unwrap();
    doc.commit();
    let before = json(&doc);
    let vv = doc.oplog_vv();
    let err = doc.revert_to(&v2).unwrap_err();
    assert!(
        matches!(&err, LoroError::ArgErr(msg) if msg.contains("Unknown(9)")),
        "{err:?}"
    );
    doc.commit();
    assert_eq!(json(&doc), before);
    assert_eq!(doc.oplog_vv(), vv);

    // Undoing the deletion is skipped without changes, like other undo steps
    // that fail to apply
    undo.undo().unwrap();
    doc.commit();
    assert_eq!(json(&doc), before);
    assert_eq!(doc.oplog_vv(), vv);
}

/// JSON updates in which a newer Loro creates an unknown container under
/// `map.k` and edits it in the same change.
fn edited_unknown_child_updates() -> String {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let counter = doc
        .get_map("map")
        .insert_container("k", loro::LoroCounter::new())
        .unwrap();
    counter.increment(2.0).unwrap();
    doc.commit();
    let json = serde_json::to_string(
        &doc.export_json_updates_without_peer_compression(&Default::default(), &doc.oplog_vv()),
    )
    .unwrap();
    assert!(json.contains("cid:0@1:Counter") && json.contains(r#""type":"counter""#));
    json.replace("cid:0@1:Counter", "cid:0@1:Unknown(9)")
        .replace(r#""type":"counter""#, r#""type":"unknown""#)
}

/// Importing or checking out a change that creates and edits an unknown
/// container used to hit `unreachable!()` in `UnknownState::apply_diff`.
#[test]
fn import_and_checkout_unknown_container_created_and_edited_together() {
    let src = LoroDoc::new();
    let _sub = src.subscribe_root(std::sync::Arc::new(|_| {}));
    src.import_json_updates(edited_unknown_child_updates().as_str())
        .unwrap();
    let expected = serde_json::json!({"map": {"k": null}});
    assert_eq!(src.get_deep_value().to_json_value(), expected);

    for mode in [ExportMode::all_updates(), ExportMode::snapshot()] {
        let doc = LoroDoc::new();
        let _sub = doc.subscribe_root(std::sync::Arc::new(|_| {}));
        doc.import(&src.export(mode).unwrap()).unwrap();
        assert_eq!(doc.get_deep_value().to_json_value(), expected);
        doc.checkout(&Default::default()).unwrap();
        assert!(doc.get_map("map").get("k").is_none());
        doc.checkout_to_latest();
        assert_eq!(doc.get_deep_value().to_json_value(), expected);
    }
}

/// Cursors only resolve in Text/List/MovableList; other container types,
/// including unknown ones, used to hit `unreachable!()`.
#[test]
fn cursor_on_non_sequence_container_is_not_found() {
    use loro::cursor::{CannotFindRelativePosition, Cursor, Side};
    let doc = LoroDoc::new();
    doc.get_map("map").insert("a", 1).unwrap();
    doc.commit();
    for container in [
        ContainerID::new_root("x", UNKNOWN),
        ContainerID::new_root("map", ContainerType::Map),
        ContainerID::new_root("tree", ContainerType::Tree),
    ] {
        for id in [None, Some(ID::new(1, 0))] {
            for side in [Side::Left, Side::Middle, Side::Right] {
                let cursor = Cursor::new(id, container.clone(), side, 0);
                assert!(matches!(
                    doc.get_cursor_pos(&cursor),
                    Err(CannotFindRelativePosition::IdNotFound)
                ));
            }
        }
    }
}

/// An undo cursor on a missing (e.g. unknown) container is left as is instead
/// of panicking when undo transforms it.
#[test]
fn undo_cursor_on_missing_container_does_not_panic() {
    use loro::cursor::{Cursor, Side};
    use loro::undo::CursorWithPos;
    let doc = LoroDoc::new();
    let mut undo = loro::UndoManager::new(&doc);
    undo.set_on_push(Some(Box::new(|_, _, _| {
        let mut meta = loro::UndoItemMeta::new();
        meta.cursors.push(CursorWithPos {
            cursor: Cursor::new(
                Some(ID::new(1, 0)),
                ContainerID::new_normal(ID::new(99, 1), UNKNOWN),
                Side::Left,
                0,
            ),
            pos: loro::cursor::AbsolutePosition {
                pos: 0,
                side: Side::Left,
            },
        });
        meta
    })));
    undo.set_on_pop(Some(Box::new(|_, _, _| {})));
    doc.get_text("text").insert(0, "a").unwrap();
    doc.commit();
    assert!(undo.undo().unwrap());
    assert!(undo.redo().unwrap());
    assert_eq!(doc.get_text("text").to_string(), "a");
}
