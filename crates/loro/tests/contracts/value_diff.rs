#[cfg(feature = "jsonpath")]
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::{borrow::Cow, collections::HashMap, iter::FromIterator};

use loro::{
    event::{Diff, ListDiffItem, MapDelta},
    ApplyDiff, CommitOptions, Container, ContainerTrait, ExportMode, Frontiers, Index, JsonSchema,
    LoroCounter, LoroDoc, LoroList, LoroMap, LoroMovableList, LoroText, LoroTree, LoroValue,
    TextDelta, ToJson, TreeID, TreeParentId, ValueOrContainer, VersionVector,
};
use pretty_assertions::assert_eq;
use rustc_hash::FxHashMap;
use serde_json::{json, Value};

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

fn text_insert(insert: &str, attributes: Option<FxHashMap<String, LoroValue>>) -> TextDelta {
    TextDelta::Insert {
        insert: insert.to_string(),
        attributes,
    }
}

fn text_retain(retain: usize, attributes: Option<FxHashMap<String, LoroValue>>) -> TextDelta {
    TextDelta::Retain { retain, attributes }
}

fn mixed_doc() -> anyhow::Result<(
    LoroDoc,
    LoroText,
    LoroList,
    LoroMovableList,
    LoroTree,
    LoroCounter,
    TreeID,
    TreeID,
    Frontiers,
    Frontiers,
)> {
    let doc = LoroDoc::new();
    doc.set_peer_id(44)?;
    doc.set_change_merge_interval(0);

    doc.set_next_commit_options(
        CommitOptions::new()
            .origin("initial")
            .timestamp(11)
            .commit_msg("seed"),
    );

    let root = doc.get_map("root");
    root.insert("title", "alpha")?;
    root.insert("count", 1_i64)?;
    root.insert("flag", true)?;
    root.insert("binary", vec![1_u8, 2, 3, 255])?;

    let body = root.insert_container("body", LoroText::new())?;
    body.insert(0, "Hello")?;

    let items = root.insert_container("items", LoroList::new())?;
    items.push("seed")?;
    let nested_map = items.insert_container(1, LoroMap::new())?;
    nested_map.insert("kind", "map")?;
    nested_map.insert("numbers", LoroValue::from(vec![7_i64, 8_i64]))?;

    let order = root.insert_container("order", LoroMovableList::new())?;
    order.push("first")?;
    order.push("second")?;
    order.push("third")?;

    let counter = root.insert_container("counter", LoroCounter::new())?;
    counter.increment(3.5)?;

    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let root_id = tree.create(TreeParentId::Root)?;
    tree.get_meta(root_id)?.insert("kind", "root")?;
    let child_id = tree.create_at(root_id, 0)?;
    tree.get_meta(child_id)?.insert("kind", "child")?;
    tree.mov(child_id, root_id)?;
    tree.delete(child_id)?;

    doc.commit();
    let first_frontiers = doc.state_frontiers();
    let initial_change = doc
        .get_change(first_frontiers.as_single().expect("single frontier"))
        .expect("initial change should exist");
    assert_eq!(initial_change.message(), "seed");
    assert_eq!(initial_change.timestamp(), 11);

    body.insert(body.len_unicode(), " world")?;
    items.push(vec![9_u8, 10])?;
    order.set(0, "first-updated")?;
    order.mov(2, 0)?;
    tree.get_meta(root_id)?.insert("label", "root-updated")?;
    counter.decrement(1.0)?;

    doc.set_next_commit_message("second");
    doc.set_next_commit_origin("merge");
    doc.set_next_commit_timestamp(22);
    doc.commit();
    let second_frontiers = doc.state_frontiers();

    let second_change = doc
        .get_change(second_frontiers.as_single().expect("single frontier"))
        .expect("second change should exist");
    assert_eq!(second_change.message(), "second");
    assert_eq!(second_change.timestamp(), 22);

    Ok((
        doc,
        body,
        items,
        order,
        tree,
        counter,
        root_id,
        child_id,
        first_frontiers,
        second_frontiers,
    ))
}

#[test]
fn mixed_state_roundtrips_through_updates_json_and_snapshots() -> anyhow::Result<()> {
    let (
        doc,
        _body,
        _items,
        _order,
        _tree,
        _counter,
        _root_id,
        _child_id,
        first_frontiers,
        second_frontiers,
    ) = mixed_doc()?;

    let tree = doc.get_tree("tree");
    let root_id = tree.roots()[0];
    let root_fractional_index = tree.fractional_index(root_id).unwrap().to_string();

    let deep = deep_json(&doc);
    assert_eq!(
        deep,
        json!({
            "root": {
                "title": "alpha",
                "count": 1,
                "flag": true,
                "binary": [1, 2, 3, 255],
                "body": "Hello world",
                "items": [
                    "seed",
                    {"kind": "map", "numbers": [7, 8]},
                    [9, 10]
                ],
                "order": ["third", "first-updated", "second"],
                "counter": 2.5,
            },
            "tree": [
                {
                    "id": root_id.to_string(),
                    "parent": null,
                    "meta": {"kind": "root", "label": "root-updated"},
                    "fractional_index": root_fractional_index,
                    "index": 0,
                    "children": []
                }
            ]
        })
    );

    assert_eq!(
        doc.get_by_str_path("root/body")
            .expect("body should resolve by string path")
            .get_deep_value()
            .to_json_value(),
        json!("Hello world")
    );
    assert_eq!(
        doc.get_by_path(&[
            Index::Key("root".into()),
            Index::Key("items".into()),
            Index::Seq(1),
        ])
        .expect("nested list item should resolve by path")
        .get_deep_value()
        .to_json_value(),
        json!({"kind": "map", "numbers": [7, 8]})
    );

    let root_path = doc
        .get_path_to_container(&tree.get_meta(root_id)?.id())
        .expect("tree meta should have a path");
    assert_eq!(
        root_path.last().map(|(_, index)| index),
        Some(&Index::Node(root_id))
    );

    let snapshot = doc.export(ExportMode::Snapshot)?;
    let snapshot_doc = LoroDoc::from_snapshot(&snapshot)?;
    assert_eq!(deep_json(&snapshot_doc), deep);

    let updates = doc.export(ExportMode::all_updates())?;
    let updates_doc = LoroDoc::new();
    updates_doc.import(&updates)?;
    assert_eq!(deep_json(&updates_doc), deep);

    let start = VersionVector::default();
    let end = doc.oplog_vv();
    let json_updates = doc.export_json_updates(&start, &end);
    let json_updates_no_peers = doc.export_json_updates_without_peer_compression(&start, &end);
    assert!(json_updates.peers.is_some());
    assert!(json_updates_no_peers.peers.is_none());

    let json_string = serde_json::to_string(&json_updates)?;
    let parsed_json: JsonSchema = json_string.as_str().try_into()?;
    assert_eq!(parsed_json.changes.len(), json_updates.changes.len());

    let json_import_doc = LoroDoc::new();
    json_import_doc.import_json_updates(json_updates.clone())?;
    assert_eq!(deep_json(&json_import_doc), deep);

    let json_import_doc_from_string = LoroDoc::new();
    json_import_doc_from_string.import_json_updates(json_string.clone())?;
    assert_eq!(deep_json(&json_import_doc_from_string), deep);

    let json_import_doc_no_peers = LoroDoc::new();
    json_import_doc_no_peers.import_json_updates(json_updates_no_peers.clone())?;
    assert_eq!(deep_json(&json_import_doc_no_peers), deep);

    let json_import_doc_no_peers_from_string = LoroDoc::new();
    json_import_doc_no_peers_from_string
        .import_json_updates(serde_json::to_string(&json_updates_no_peers)?)?;
    assert_eq!(deep_json(&json_import_doc_no_peers_from_string), deep);

    let peer = doc.peer_id();
    let first_vv = doc
        .frontiers_to_vv(&first_frontiers)
        .expect("first frontiers should convert");
    let second_vv = doc
        .frontiers_to_vv(&second_frontiers)
        .expect("second frontiers should convert");
    let first_end = *first_vv.get(&peer).expect("peer should exist in first vv");
    let second_end = *second_vv
        .get(&peer)
        .expect("peer should exist in second vv");
    let first = doc.export(ExportMode::updates_in_range(vec![loro::IdSpan::new(
        peer, 0, first_end,
    )]))?;
    let second = doc.export(ExportMode::updates_in_range(vec![loro::IdSpan::new(
        peer, first_end, second_end,
    )]))?;
    let batch_doc = LoroDoc::new();
    let status = batch_doc.import_batch(&[second.clone(), first.clone()])?;
    assert!(status.pending.is_none());
    let status = batch_doc.import_batch(&[first, second])?;
    assert!(status.pending.is_none());
    assert_eq!(deep_json(&batch_doc), deep);

    Ok(())
}

#[test]
fn loro_value_apply_diff_and_apply_path_cover_nested_container_and_tree_contracts(
) -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let attached_map = doc.get_map("attached_map");
    attached_map.insert("kind", "map")?;
    let attached_text = doc.get_text("attached_text");
    attached_text.insert(0, "seed")?;
    doc.commit();

    let mut value = LoroValue::from(HashMap::<String, LoroValue>::new());

    let title_path = vec![Index::Key("doc".into()), Index::Key("title".into())].into();
    value.apply(
        &title_path,
        &[Diff::Text(vec![text_insert(
            "Hello",
            Some(FxHashMap::from_iter([("bold".to_string(), true.into())])),
        )])
        .into()],
    );

    let items_path = vec![Index::Key("doc".into()), Index::Key("items".into())].into();
    value.apply(
        &items_path,
        &[Diff::List(vec![ListDiffItem::Insert {
            insert: vec![
                ValueOrContainer::Value(1_i64.into()),
                ValueOrContainer::Container(Container::Map(attached_map.clone())),
                ValueOrContainer::Container(Container::Text(attached_text.clone())),
            ],
            is_move: false,
        }])
        .into()],
    );

    let meta_path = vec![Index::Key("doc".into()), Index::Key("meta".into())].into();
    value.apply(
        &meta_path,
        &[Diff::Map(MapDelta {
            updated: FxHashMap::from_iter([
                (
                    Cow::Borrowed("keep"),
                    Some(ValueOrContainer::Value("yes".into())),
                ),
                (
                    Cow::Borrowed("container"),
                    Some(ValueOrContainer::Container(Container::Text(
                        attached_text.clone(),
                    ))),
                ),
                (Cow::Borrowed("drop"), None),
            ]),
        })
        .into()],
    );

    assert_eq!(
        value.to_json_value(),
        json!({
            "doc": {
                "title": "Hello",
                "items": [1, {}, ""],
                "meta": {
                    "keep": "yes",
                    "container": ""
                }
            }
        })
    );

    let mut text_value = LoroValue::from("abcdef");
    text_value.apply_diff_shallow(&[Diff::Text(vec![
        text_retain(
            2,
            Some(FxHashMap::from_iter([("italic".to_string(), true.into())])),
        ),
        text_insert(
            "XY",
            Some(FxHashMap::from_iter([("bold".to_string(), true.into())])),
        ),
        TextDelta::Delete { delete: 2 },
        text_retain(2, None),
    ])
    .into()]);
    assert_eq!(text_value.to_json_value(), json!("abXYef"));

    let mut list_value = LoroValue::from(vec![1_i64, 2_i64, 3_i64, 4_i64]);
    list_value.apply_diff(&[Diff::List(vec![
        ListDiffItem::Retain { retain: 1 },
        ListDiffItem::Delete { delete: 1 },
        ListDiffItem::Insert {
            insert: vec![
                ValueOrContainer::Value(9_i64.into()),
                ValueOrContainer::Value(LoroValue::from(vec![7_i64, 8_i64])),
                ValueOrContainer::Container(Container::Map(attached_map.clone())),
            ],
            is_move: false,
        },
        ListDiffItem::Retain { retain: 1 },
    ])
    .into()]);
    assert_eq!(list_value.to_json_value(), json!([1, 9, [7, 8], {}, 3, 4]));

    let mut map_value = LoroValue::from(HashMap::<String, LoroValue>::new());
    map_value.apply_diff(&[Diff::Map(MapDelta {
        updated: FxHashMap::from_iter([
            (
                Cow::Borrowed("flag"),
                Some(ValueOrContainer::Value(true.into())),
            ),
            (
                Cow::Borrowed("nested"),
                Some(ValueOrContainer::Container(Container::Text(
                    attached_text.clone(),
                ))),
            ),
            (Cow::Borrowed("removed"), None),
        ]),
    })
    .into()]);
    assert_eq!(
        map_value.to_json_value(),
        json!({"flag": true, "nested": ""})
    );

    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let root = tree.create(TreeParentId::Root)?;
    tree.get_meta(root)?.insert("kind", "root")?;
    let child = tree.create_at(root, 0)?;
    tree.get_meta(child)?.insert("kind", "child")?;
    doc.commit();

    let mut tree_value = tree.get_value_with_meta();
    tree_value.apply(
        &vec![Index::Node(root)].into(),
        &[Diff::Map(MapDelta {
            updated: FxHashMap::from_iter([(
                Cow::Borrowed("label"),
                Some(ValueOrContainer::Value("updated".into())),
            )]),
        })
        .into()],
    );
    assert_eq!(
        tree_value.to_json_value(),
        json!([
            {
                "id": root.to_string(),
                "parent": null,
                "meta": {"kind": "root", "label": "updated"},
                "fractional_index": tree.fractional_index(root).unwrap().to_string(),
                "index": 0,
                "children": [
                    {
                        "id": child.to_string(),
                        "parent": root.to_string(),
                        "meta": {"kind": "child"},
                        "fractional_index": tree.fractional_index(child).unwrap().to_string(),
                        "index": 0,
                        "children": []
                    }
                ]
            }
        ])
    );

    tree.delete(child)?;
    let deleted_before = tree_value.clone();
    tree_value.apply(
        &vec![Index::Node(child)].into(),
        &[Diff::Map(MapDelta {
            updated: FxHashMap::from_iter([(
                Cow::Borrowed("ghost"),
                Some(ValueOrContainer::Value(true.into())),
            )]),
        })
        .into()],
    );
    assert_eq!(tree_value, deleted_before);

    Ok(())
}

#[test]
#[cfg(feature = "jsonpath")]
fn jsonpath_value_length_and_invalid_function_errors_match_contract() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let root = doc.get_map("root");
    root.insert("title", "Hello")?;
    root.insert("note", "Short")?;
    let list = root.insert_container("items", LoroList::new())?;
    let first = list.insert_container(0, LoroMap::new())?;
    first.insert("name", "alpha")?;
    let second = list.insert_container(1, LoroMap::new())?;
    second.insert("name", "beta")?;
    doc.commit();

    let title_nodes = doc.jsonpath("$.root.title")?;
    assert_eq!(title_nodes.len(), 1);
    assert_eq!(
        title_nodes[0].get_deep_value().to_json_value(),
        json!("Hello")
    );
    assert_eq!(
        doc.jsonpath("$.root.items[?(count(@.name) == 1 && length(value($.root.title)) == 5)]")?
            .len(),
        2
    );
    assert_eq!(
        doc.jsonpath("$.root.items[?value(@.name) == 'alpha'].name")?
            .into_iter()
            .map(|value| value.get_deep_value().to_json_value())
            .collect::<Vec<_>>(),
        vec![json!("alpha")]
    );
    assert_eq!(
        doc.jsonpath("$.root.items[?(@.name in ['alpha', 'beta'])].name")?
            .into_iter()
            .map(|value| value.get_deep_value().to_json_value())
            .collect::<Vec<_>>(),
        vec![json!("alpha"), json!("beta")]
    );

    for invalid in [
        "$.root.items[?foo(@.name)]",
        "$.root.items[?foo(@.name) == true]",
    ] {
        assert!(
            doc.jsonpath(invalid).is_err(),
            "{invalid} should be rejected"
        );
    }

    for invalid in [
        "$.root.items[?match(@.name)]",
        "$.root.items[?match(@.name, 'alpha') == true]",
    ] {
        assert!(
            doc.jsonpath(invalid).is_err(),
            "{invalid} should be rejected"
        );
    }

    for panicky in [
        "$.root.items[?search(@.name, 'alpha')]",
        "$.root.items[?search(@.name, 'alpha') == true]",
    ] {
        let result = catch_unwind(AssertUnwindSafe(|| doc.jsonpath(panicky)));
        match result {
            Ok(Ok(_)) => panic!("{panicky} should not succeed until implemented"),
            Ok(Err(_)) | Err(_) => {}
        }
    }

    Ok(())
}
