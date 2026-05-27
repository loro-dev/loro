use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    sync::Arc,
};

use loro::{
    ContainerID, ContainerTrait, ContainerType, ExportMode, IdSpan, JsonListOp, JsonMapOp,
    JsonMovableListOp, JsonOpContent, JsonTextOp, JsonTreeOp, LoroDoc, LoroList, LoroMap,
    LoroMapValue, LoroMovableList, LoroText, LoroTree, LoroValue, ToJson, TreeID, ValueOrContainer,
    VersionVector, ID,
};
use pretty_assertions::assert_eq;
use rustc_hash::FxHashMap;
use serde_json::{json, Value};

fn nested_value() -> LoroValue {
    let nested_map = HashMap::from([
        ("flag".to_string(), true.into()),
        ("bytes".to_string(), vec![9u8, 8, 7].into()),
    ]);

    LoroValue::from(vec![
        LoroValue::Null,
        false.into(),
        1_i64.into(),
        2.5_f64.into(),
        "leaf".into(),
        vec![4u8, 5, 6].into(),
        LoroValue::from(nested_map),
    ])
}

fn value_json(value: &LoroValue) -> Value {
    value.to_json_value()
}

fn value_hash(value: &LoroValue) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn nested_container<T: ContainerTrait>(map: &LoroMap, key: &str) -> T {
    let value = map
        .get(key)
        .unwrap_or_else(|| panic!("missing nested container {key}"));
    let container = match value {
        ValueOrContainer::Container(container) => container,
        ValueOrContainer::Value(_) => panic!("expected nested container for key {key}"),
    };
    T::try_from_container(container).expect("nested container type should match")
}

fn build_value_doc() -> anyhow::Result<(
    LoroDoc,
    LoroMap,
    LoroList,
    LoroMovableList,
    LoroText,
    LoroTree,
)> {
    let doc = LoroDoc::new();
    doc.set_peer_id(19)?;

    let root = doc.get_map("root");
    root.insert("null", LoroValue::Null)?;
    root.insert("bool", true)?;
    root.insert("i64", i64::MAX - 17)?;
    root.insert("double", -1234.5)?;
    root.insert("string", "A😀B")?;
    root.insert("binary", vec![0u8, 1, 2, 255])?;
    root.insert("nested", nested_value())?;

    let text = root.insert_container("text", LoroText::new())?;
    text.insert(0, "Hello 🌍")?;
    text.mark(0..5, "bold", true)?;
    text.unmark(1..4, "bold")?;

    let list = root.insert_container("list", LoroList::new())?;
    list.insert(0, LoroValue::Null)?;
    list.insert(1, vec![1u8, 2, 3])?;
    let list_nested = list.insert_container(2, LoroMap::new())?;
    list_nested.insert("title", "nested map")?;
    list_nested.insert("payload", nested_value())?;
    list.insert(3, false)?;
    list.delete(3, 1)?;

    let mlist = root.insert_container("mlist", LoroMovableList::new())?;
    mlist.insert(0, "first")?;
    mlist.insert(1, 2_i64)?;
    let mlist_nested = mlist.insert_container(2, LoroText::new())?;
    mlist_nested.insert(0, "child")?;
    mlist.set(1, vec![7u8, 8, 9])?;
    mlist.mov(0, 2)?;

    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let root_a = tree.create(None)?;
    let root_b = tree.create(None)?;
    tree.get_meta(root_a)?.insert("title", "root-a")?;
    tree.get_meta(root_b)?.insert("title", "root-b")?;
    tree.mov(root_b, root_a)?;
    let child = tree.create_at(root_a, 0)?;
    tree.get_meta(child)?.insert("title", "child")?;
    tree.delete(child)?;

    doc.commit();
    Ok((doc, root, list, mlist, text, tree))
}

#[test]
fn snapshot_state_only_decode_reads_current_value_without_history() -> anyhow::Result<()> {
    let (doc, root, list, _, _, _) = build_value_doc()?;
    let snapshot = doc.export(ExportMode::Snapshot)?;
    let restored = LoroDoc::from_snapshot(&snapshot)?;

    assert_eq!(
        LoroDoc::decode_snapshot_state_only_value(&snapshot)?,
        restored.get_deep_value()
    );
    assert_eq!(
        LoroDoc::decode_snapshot_state_only_value(&doc.export(ExportMode::StateOnly(None))?)?,
        restored.get_deep_value()
    );

    let nested_map_id = match list.get(2).expect("list should contain a nested map") {
        ValueOrContainer::Container(container) => container.id().to_string(),
        ValueOrContainer::Value(_) => panic!("expected nested map container"),
    };
    let mirror_value = LoroDoc::decode_snapshot_state_only_mirror_value(&snapshot)?;
    let mirror_json = mirror_value.to_json_value();
    assert_eq!(mirror_value, restored.get_deep_value_with_map_id());

    assert_eq!(mirror_json["root"]["$cid"], json!(root.id().to_string()));
    assert_eq!(mirror_json["root"]["list"][2]["$cid"], json!(nested_map_id));
    assert_eq!(mirror_json["root"]["text"], json!("Hello 🌍"));

    Ok(())
}

#[test]
fn loro_value_contracts_roundtrip_for_scalars_collections_and_containers() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(7)?;
    let map = doc.get_map("root");
    let text = map.insert_container("child", LoroText::new())?;
    text.insert(0, "x")?;
    doc.commit();

    let container_value = LoroValue::from(text.id());
    let container_json = serde_json::to_value(&container_value)?;
    assert_eq!(container_json, Value::String(format!("🦜:{}", text.id())));
    assert_eq!(
        serde_json::from_value::<LoroValue>(container_json.clone())?,
        container_value
    );
    assert_eq!(
        value_json(&serde_json::from_value::<LoroValue>(container_json)?),
        Value::String(format!("🦜:{}", text.id()))
    );

    let null = LoroValue::Null;
    let boolean = LoroValue::from(false);
    let i64_value = LoroValue::from(i64::MAX - 3);
    let float_value = LoroValue::from(-12.25_f64);
    let string_value = LoroValue::from("hello");
    let binary_value = LoroValue::from(vec![1u8, 2, 3, 255]);
    let list_value = LoroValue::from(vec![
        LoroValue::Null,
        true.into(),
        42_i64.into(),
        3.5_f64.into(),
        "text".into(),
        vec![4_i64, 5_i64].into(),
    ]);
    let map_value = LoroValue::from(HashMap::from([
        ("null".to_string(), LoroValue::Null),
        ("flag".to_string(), true.into()),
        ("count".to_string(), 9_i64.into()),
        ("ratio".to_string(), 1.25_f64.into()),
        ("label".to_string(), "map".into()),
        ("nums".to_string(), vec![7_i64, 8_i64, 9_i64].into()),
        ("list".to_string(), list_value.clone()),
    ]));

    assert_eq!(serde_json::to_value(&null)?, Value::Null);
    assert_eq!(serde_json::from_value::<LoroValue>(json!(null))?, null);
    assert_eq!(serde_json::to_value(&boolean)?, Value::Bool(false));
    assert_eq!(serde_json::from_value::<LoroValue>(json!(false))?, boolean);
    assert_eq!(serde_json::to_value(&i64_value)?, json!(i64::MAX - 3));
    assert_eq!(
        serde_json::from_value::<LoroValue>(json!(i64::MAX - 3))?,
        i64_value
    );
    let large_u64_value = LoroValue::Double(u64::MAX as f64);
    assert_eq!(
        serde_json::from_value::<LoroValue>(json!(u64::MAX))?,
        large_u64_value
    );
    assert_eq!(LoroValue::from(json!(u64::MAX)), large_u64_value);
    assert_eq!(serde_json::to_value(&float_value)?, json!(-12.25));
    assert_eq!(
        serde_json::from_value::<LoroValue>(json!(-12.25))?,
        float_value
    );
    for non_finite in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let non_finite_value = LoroValue::Double(non_finite);
        assert_eq!(serde_json::to_value(&non_finite_value)?, Value::Null);
        assert_eq!(value_json(&non_finite_value), Value::Null);
        assert_eq!(Value::from(non_finite_value), Value::Null);
    }
    assert_eq!(LoroValue::Double(f64::NAN), LoroValue::Double(f64::NAN));
    assert_eq!(LoroValue::Double(0.0), LoroValue::Double(-0.0));
    assert_eq!(
        value_hash(&LoroValue::Double(0.0)),
        value_hash(&LoroValue::Double(-0.0))
    );
    let mut compact_map = FxHashMap::default();
    compact_map.insert("a".to_string(), 1_i64.into());
    compact_map.insert("b".to_string(), 2_i64.into());
    let mut sparse_map = FxHashMap::default();
    for i in 0..64 {
        sparse_map.insert(format!("padding-{i}"), LoroValue::from(i));
    }
    for i in 0..64 {
        sparse_map.remove(&format!("padding-{i}"));
    }
    sparse_map.insert("b".to_string(), 2_i64.into());
    sparse_map.insert("a".to_string(), 1_i64.into());
    let compact_value = LoroValue::Map(LoroMapValue::from(compact_map));
    let sparse_value = LoroValue::Map(LoroMapValue::from(sparse_map));
    assert_eq!(compact_value, sparse_value);
    assert_eq!(value_hash(&compact_value), value_hash(&sparse_value));
    assert_eq!(serde_json::to_value(&string_value)?, json!("hello"));
    assert_eq!(
        serde_json::from_value::<LoroValue>(json!("hello"))?,
        string_value
    );
    assert_eq!(serde_json::to_value(&binary_value)?, json!([1, 2, 3, 255]));
    assert_eq!(
        serde_json::to_value(&list_value)?,
        json!([null, true, 42, 3.5, "text", [4, 5]])
    );
    assert_eq!(
        serde_json::from_value::<LoroValue>(json!([null, true, 42, 3.5, "text", [4, 5]]))?,
        list_value
    );
    assert_eq!(
        serde_json::to_value(&map_value)?,
        json!({
            "null": null,
            "flag": true,
            "count": 9,
            "ratio": 1.25,
            "label": "map",
            "nums": [7, 8, 9],
            "list": [null, true, 42, 3.5, "text", [4, 5]],
        })
    );
    assert_eq!(
        serde_json::from_value::<LoroValue>(json!({
            "null": null,
            "flag": true,
            "count": 9,
            "ratio": 1.25,
            "label": "map",
            "nums": [7, 8, 9],
            "list": [null, true, 42, 3.5, "text", [4, 5]],
        }))?,
        map_value
    );

    assert_eq!(bool::try_from(LoroValue::from(false)).unwrap(), false);
    assert_eq!(f64::try_from(LoroValue::from(1.5_f64)).unwrap(), 1.5);
    assert_eq!(i32::try_from(LoroValue::from(123_i64)).unwrap(), 123);
    assert!(i32::try_from(LoroValue::from(i64::from(i32::MAX) + 1)).is_err());
    assert!(i32::try_from(LoroValue::from(i64::from(i32::MIN) - 1)).is_err());
    assert_eq!(
        ContainerID::try_from(container_value.clone()).unwrap(),
        text.id()
    );

    let bytes_arc: Arc<Vec<u8>> = binary_value.clone().try_into().unwrap();
    assert_eq!(bytes_arc.as_ref(), &vec![1, 2, 3, 255]);
    let string_arc: Arc<String> = string_value.clone().try_into().unwrap();
    assert_eq!(string_arc.as_ref(), "hello");
    let list_arc: Arc<Vec<LoroValue>> = list_value.clone().try_into().unwrap();
    assert_eq!(
        list_arc.as_ref(),
        &list_value.clone().into_list().unwrap().unwrap()
    );

    assert_eq!(map_value.get_by_key("flag"), Some(&LoroValue::from(true)));
    assert_eq!(
        list_value.get_by_index(-1),
        Some(&LoroValue::from(vec![4_i64, 5_i64]))
    );
    assert_eq!(list_value.get_by_index(0), Some(&LoroValue::Null));
    assert_eq!(list_value.get_by_index(-7), None);
    assert_eq!(list_value.get_by_index(isize::MIN), None);
    assert_eq!(LoroValue::Null.get_by_index(-1), None);
    assert_eq!(list_value[5], LoroValue::from(vec![4_i64, 5_i64]));
    assert_eq!(map_value["missing"], LoroValue::Null);
    assert_eq!(list_value[99], LoroValue::Null);

    let mut list_children = Vec::new();
    list_value.visit_children(&mut |child| list_children.push(child.clone()));
    assert_eq!(list_children.len(), 6);

    let mut map_children = Vec::new();
    map_value.visit_children(&mut |child| map_children.push(child.clone()));
    assert_eq!(map_children.len(), 7);

    let mut scalar_children = 0;
    LoroValue::Null.visit_children(&mut |_| scalar_children += 1);
    assert_eq!(scalar_children, 0);

    assert!(!LoroValue::Null.is_false());
    assert!(LoroValue::from(false).is_false());
    assert!(!LoroValue::from(true).is_false());
    assert!(LoroValue::from(Vec::<u8>::new()).is_empty_collection());
    assert!(LoroValue::from(String::new()).is_empty_collection());
    assert!(LoroValue::from(Vec::<LoroValue>::new()).is_empty_collection());
    assert!(LoroValue::from(HashMap::<String, LoroValue>::new()).is_empty_collection());
    assert!(!LoroValue::Null.is_empty_collection());

    let shallow_depth = nested_value();
    assert_eq!(shallow_depth.get_depth(), 2);
    assert!(!shallow_depth.is_too_deep());
    let mut too_deep = LoroValue::Null;
    for _ in 0..129 {
        too_deep = LoroValue::from(vec![too_deep]);
    }
    assert!(too_deep.is_too_deep());

    Ok(())
}

#[test]
fn json_updates_roundtrip_nested_values_and_peer_compression() -> anyhow::Result<()> {
    let (doc, root, list, mlist, text, tree) = build_value_doc()?;
    let start = VersionVector::default();
    let end = doc.oplog_vv();

    let compressed = doc.export_json_updates(&start, &end);
    let uncompressed = doc.export_json_updates_without_peer_compression(&start, &end);

    assert_eq!(compressed.schema_version, 1);
    assert_eq!(uncompressed.schema_version, 1);
    assert!(compressed.peers.is_some());
    assert!(uncompressed.peers.is_none());
    assert_eq!(compressed.changes.len(), uncompressed.changes.len());

    let end_counter = *end.get(&19).expect("peer 19 should have updates");
    assert_eq!(
        serde_json::to_value(doc.export_json_in_id_span(IdSpan::new(19, 0, end_counter)))?,
        serde_json::to_value(&uncompressed.changes)?
    );

    let imported = LoroDoc::new();
    imported.import_json_updates(compressed.clone())?;
    let imported_root = imported.get_map("root");
    let imported_list: LoroList = nested_container(&imported_root, "list");
    let imported_mlist: LoroMovableList = nested_container(&imported_root, "mlist");
    let imported_text: LoroText = nested_container(&imported_root, "text");
    let imported_tree = imported.get_tree("tree");

    assert_eq!(
        imported_root.get_value().to_json_value(),
        root.get_value().to_json_value()
    );
    assert_eq!(
        imported_root.get_deep_value().to_json_value(),
        root.get_deep_value().to_json_value()
    );
    assert_eq!(
        imported_list.get_deep_value().to_json_value(),
        list.get_deep_value().to_json_value()
    );
    assert_eq!(
        imported_mlist.get_deep_value().to_json_value(),
        mlist.get_deep_value().to_json_value()
    );
    assert_eq!(imported_text.to_string(), text.to_string());
    assert_eq!(
        imported_text.get_richtext_value().to_json_value(),
        text.get_richtext_value().to_json_value()
    );
    assert_eq!(
        imported_tree.get_value_with_meta().to_json_value(),
        tree.get_value_with_meta().to_json_value()
    );

    let imported_without_peer = LoroDoc::new();
    imported_without_peer.import_json_updates(uncompressed.clone())?;
    assert_eq!(
        imported_without_peer
            .get_map("root")
            .get_deep_value()
            .to_json_value(),
        root.get_deep_value().to_json_value()
    );
    assert_eq!(
        imported_without_peer
            .get_tree("tree")
            .get_value_with_meta()
            .to_json_value(),
        tree.get_value_with_meta().to_json_value()
    );

    Ok(())
}

#[test]
fn import_json_updates_accepts_reordered_op_fields() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(76)?;
    let root = doc.get_map("root");
    root.insert("key", "value")?;
    doc.commit();

    let json = doc
        .export_json_updates_without_peer_compression(&VersionVector::default(), &doc.oplog_vv());
    let value = serde_json::to_value(json)?;
    let change = &value["changes"][0];
    let op = &change["ops"][0];
    let raw_json = format!(
        r#"{{"schema_version":{},"start_version":{},"peers":{},"changes":[{{"id":{},"timestamp":{},"deps":{},"lamport":{},"msg":{},"ops":[{{"counter":{},"content":{},"container":{}}}]}}]}}"#,
        serde_json::to_string(&value["schema_version"])?,
        serde_json::to_string(&value["start_version"])?,
        serde_json::to_string(&value["peers"])?,
        serde_json::to_string(&change["id"])?,
        serde_json::to_string(&change["timestamp"])?,
        serde_json::to_string(&change["deps"])?,
        serde_json::to_string(&change["lamport"])?,
        serde_json::to_string(&change["msg"])?,
        serde_json::to_string(&op["counter"])?,
        serde_json::to_string(&op["content"])?,
        serde_json::to_string(&op["container"])?,
    );

    let imported = LoroDoc::new();
    imported.import_json_updates(raw_json)?;

    assert_eq!(
        imported.get_map("root").get_deep_value().to_json_value(),
        root.get_deep_value().to_json_value()
    );

    Ok(())
}

#[test]
fn import_json_updates_rejects_unsupported_schema_version() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.get_map("root").insert("key", "value")?;
    doc.commit();

    let mut json = doc
        .export_json_updates_without_peer_compression(&VersionVector::default(), &doc.oplog_vv());
    json.schema_version = 2;

    let err = LoroDoc::new().import_json_updates(json).unwrap_err();
    assert!(
        err.to_string().contains("schema version"),
        "expected schema version validation error, got {err:?}"
    );

    Ok(())
}

#[test]
fn export_json_in_id_span_clamps_negative_counter_ranges() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(92)?;
    doc.get_text("text").insert(0, "a")?;
    doc.commit();

    let changes = doc.export_json_in_id_span(IdSpan::new(92, i32::MIN, 1));
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].id.counter, 0);

    let changes = doc.export_json_in_id_span(IdSpan::new(92, i32::MIN, -1));
    assert!(changes.is_empty());

    let changes = doc.export_json_in_id_span(IdSpan::new(92, 1, i32::MIN));
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].id.counter, 0);

    Ok(())
}

#[test]
fn export_json_updates_clamps_negative_version_ranges() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(93)?;
    doc.get_text("text").insert(0, "a")?;
    doc.commit();

    let mut negative_start = VersionVector::new();
    negative_start.insert(doc.peer_id(), i32::MIN);
    let json = doc.export_json_updates(&negative_start, &doc.oplog_vv());
    let restored = LoroDoc::new();
    restored.import_json_updates(json)?;
    assert_eq!(restored.get_text("text").to_string(), "a");

    let mut negative_end = VersionVector::new();
    negative_end.insert(doc.peer_id(), -1);
    let json = doc.export_json_updates(&VersionVector::default(), &negative_end);
    let restored = LoroDoc::new();
    restored.import_json_updates(json)?;
    assert_eq!(restored.get_text("text").to_string(), "");

    Ok(())
}

#[test]
fn import_json_updates_rejects_negative_op_counters() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.get_map("root").insert("key", "value")?;
    doc.commit();

    let mut json = doc
        .export_json_updates_without_peer_compression(&VersionVector::default(), &doc.oplog_vv());
    json.changes[0].id.counter = -1;
    json.changes[0].ops[0].counter = -1;

    let err = LoroDoc::new().import_json_updates(json).unwrap_err();
    assert!(
        err.to_string().contains("counter"),
        "expected counter validation error, got {err:?}"
    );

    Ok(())
}

#[test]
fn import_json_updates_rejects_negative_dependency_counters() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(77)?;
    doc.get_map("root").insert("key", "value")?;
    doc.commit();

    let mut json = doc
        .export_json_updates_without_peer_compression(&VersionVector::default(), &doc.oplog_vv());
    json.changes[0].deps.push(ID::new(77, -1));

    let err = LoroDoc::new().import_json_updates(json).unwrap_err();
    assert!(
        err.to_string().contains("counter"),
        "expected counter validation error, got {err:?}"
    );

    Ok(())
}

#[test]
fn json_update_schema_covers_list_map_text_tree_and_movable_list_ops() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(31)?;

    let root = doc.get_map("schema");
    root.insert("plain", 1_i64)?;
    root.insert("flag", true)?;
    root.insert("payload", vec![0u8, 1, 2])?;

    let list = root.insert_container("list", LoroList::new())?;
    list.insert(0, "alpha")?;
    list.insert(1, vec![1u8, 2, 3])?;
    list.delete(1, 1)?;

    let map_child = root.insert_container("map_child", LoroMap::new())?;
    map_child.insert("title", "nested")?;
    map_child.delete("title")?;

    let text = root.insert_container("text", LoroText::new())?;
    text.insert(0, "hello")?;
    text.mark(0..2, "bold", true)?;
    text.unmark(0..2, "bold")?;
    text.delete(0, 1)?;

    let mlist = root.insert_container("mlist", LoroMovableList::new())?;
    mlist.insert(0, "a")?;
    mlist.insert(1, "b")?;
    mlist.set(1, vec![4u8, 5])?;
    mlist.mov(0, 1)?;

    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let a = tree.create(None)?;
    let b = tree.create(None)?;
    tree.get_meta(a)?.insert("title", "A")?;
    tree.get_meta(b)?.insert("title", "B")?;
    tree.mov(b, a)?;
    tree.delete(b)?;

    doc.commit();

    let end = doc.oplog_vv();
    let end_counter = *end.get(&31).expect("peer 31 should have updates");
    let changes = doc.export_json_in_id_span(IdSpan::new(31, 0, end_counter));

    assert!(!changes.is_empty());
    assert_eq!(
        serde_json::to_value(&changes)?,
        serde_json::to_value(
            &doc.export_json_updates_without_peer_compression(&VersionVector::default(), &end)
                .changes
        )?
    );

    let mut saw_list_insert = false;
    let mut saw_list_delete = false;
    let mut saw_map_insert = false;
    let mut saw_map_delete = false;
    let mut saw_text_insert = false;
    let mut saw_text_mark = false;
    let mut saw_text_mark_end = false;
    let mut saw_text_delete = false;
    let mut saw_mlist_insert = false;
    let mut saw_mlist_set = false;
    let mut saw_mlist_move = false;
    let mut saw_tree_create = false;
    let mut saw_tree_move = false;
    let mut saw_tree_delete = false;

    for change in &changes {
        for op in &change.ops {
            match &op.content {
                JsonOpContent::List(JsonListOp::Insert { value, .. }) => {
                    saw_list_insert = true;
                    assert!(value.iter().any(|v| v == &LoroValue::from("alpha")));
                }
                JsonOpContent::List(JsonListOp::Delete { .. }) => {
                    saw_list_delete = true;
                }
                JsonOpContent::Map(JsonMapOp::Insert { key, value }) => {
                    if key == "plain" {
                        saw_map_insert = true;
                        assert_eq!(*value, LoroValue::from(1_i64));
                    }
                }
                JsonOpContent::Map(JsonMapOp::Delete { key }) => {
                    if key == "title" {
                        saw_map_delete = true;
                    }
                }
                JsonOpContent::Text(JsonTextOp::Insert { text, .. }) => {
                    saw_text_insert = true;
                    assert_eq!(text, "hello");
                }
                JsonOpContent::Text(JsonTextOp::Mark { style_key, .. }) => {
                    if style_key == "bold" {
                        saw_text_mark = true;
                    }
                }
                JsonOpContent::Text(JsonTextOp::MarkEnd) => {
                    saw_text_mark_end = true;
                }
                JsonOpContent::Text(JsonTextOp::Delete { .. }) => {
                    saw_text_delete = true;
                }
                JsonOpContent::MovableList(JsonMovableListOp::Insert { value, .. }) => {
                    saw_mlist_insert = true;
                    assert!(value.iter().any(|v| v == &LoroValue::from("a")));
                }
                JsonOpContent::MovableList(JsonMovableListOp::Set { value, .. }) => {
                    saw_mlist_set = true;
                    assert_eq!(*value, LoroValue::from(vec![4u8, 5]));
                }
                JsonOpContent::MovableList(JsonMovableListOp::Move { .. }) => {
                    saw_mlist_move = true;
                }
                JsonOpContent::Tree(JsonTreeOp::Create { .. }) => {
                    saw_tree_create = true;
                }
                JsonOpContent::Tree(JsonTreeOp::Move { .. }) => {
                    saw_tree_move = true;
                }
                JsonOpContent::Tree(JsonTreeOp::Delete { .. }) => {
                    saw_tree_delete = true;
                }
                _ => {}
            }
        }
    }

    assert!(saw_list_insert);
    assert!(saw_list_delete);
    assert!(saw_map_insert);
    assert!(saw_map_delete);
    assert!(saw_text_insert);
    assert!(saw_text_mark);
    assert!(saw_text_mark_end);
    assert!(saw_text_delete);
    assert!(saw_mlist_insert);
    assert!(saw_mlist_set);
    assert!(saw_mlist_move);
    assert!(saw_tree_create);
    assert!(saw_tree_move);
    assert!(saw_tree_delete);

    let compressed = doc.export_json_updates(&VersionVector::default(), &end);
    assert!(compressed.peers.is_some());
    assert_eq!(compressed.changes.len(), changes.len());

    let replay = LoroDoc::new();
    replay.import_json_updates(compressed.clone())?;
    assert_eq!(
        replay.get_map("schema").get_deep_value().to_json_value(),
        doc.get_map("schema").get_deep_value().to_json_value()
    );
    assert_eq!(
        replay
            .get_tree("tree")
            .get_value_with_meta()
            .to_json_value(),
        doc.get_tree("tree").get_value_with_meta().to_json_value()
    );

    Ok(())
}

#[test]
fn import_json_updates_rejects_non_contiguous_op_counters() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(71)?;
    doc.get_map("root").insert("key", "value")?;
    doc.commit();

    let mut json = doc
        .export_json_updates_without_peer_compression(&VersionVector::default(), &doc.oplog_vv());
    json.changes[0].ops[0].counter += 1;

    let err = LoroDoc::new().import_json_updates(json).unwrap_err();
    assert!(
        err.to_string().contains("op counter"),
        "expected op counter validation error, got {err:?}"
    );

    Ok(())
}

#[test]
fn import_json_updates_rejects_mismatched_created_container_id() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(72)?;
    doc.get_map("root")
        .insert_container("child", LoroList::new())?;
    doc.commit();

    let mut json = doc
        .export_json_updates_without_peer_compression(&VersionVector::default(), &doc.oplog_vv());
    for op in &mut json.changes[0].ops {
        if let JsonOpContent::Map(JsonMapOp::Insert {
            value: LoroValue::Container(id),
            ..
        }) = &mut op.content
        {
            *id = ContainerID::new_normal(ID::new(72, op.counter + 10), ContainerType::List);
            break;
        }
    }

    let err = LoroDoc::new().import_json_updates(json).unwrap_err();
    assert!(
        err.to_string().contains("container id"),
        "expected container id validation error, got {err:?}"
    );

    Ok(())
}

#[test]
fn import_json_updates_rejects_mismatched_list_created_container_ids() -> anyhow::Result<()> {
    let list_doc = LoroDoc::new();
    list_doc.set_peer_id(74)?;
    list_doc
        .get_list("list")
        .insert_container(0, LoroMap::new())?;
    list_doc.commit();

    let mut list_json = list_doc.export_json_updates_without_peer_compression(
        &VersionVector::default(),
        &list_doc.oplog_vv(),
    );
    for op in &mut list_json.changes[0].ops {
        if let JsonOpContent::List(JsonListOp::Insert { value, .. }) = &mut op.content {
            if let Some(LoroValue::Container(id)) = value.first_mut() {
                *id = ContainerID::new_normal(ID::new(74, op.counter + 10), ContainerType::Map);
                break;
            }
        }
    }

    let err = LoroDoc::new().import_json_updates(list_json).unwrap_err();
    assert!(
        err.to_string().contains("container id"),
        "expected list container id validation error, got {err:?}"
    );

    let movable_doc = LoroDoc::new();
    movable_doc.set_peer_id(75)?;
    let movable = movable_doc.get_movable_list("movable");
    movable.insert(0, "seed")?;
    movable.set_container(0, LoroText::new())?;
    movable_doc.commit();

    let mut movable_json = movable_doc.export_json_updates_without_peer_compression(
        &VersionVector::default(),
        &movable_doc.oplog_vv(),
    );
    for op in &mut movable_json.changes[0].ops {
        if let JsonOpContent::MovableList(JsonMovableListOp::Set {
            value: LoroValue::Container(id),
            ..
        }) = &mut op.content
        {
            *id = ContainerID::new_normal(ID::new(75, op.counter + 10), ContainerType::Text);
            break;
        }
    }

    let err = LoroDoc::new()
        .import_json_updates(movable_json)
        .unwrap_err();
    assert!(
        err.to_string().contains("container id"),
        "expected movable list container id validation error, got {err:?}"
    );

    Ok(())
}

#[test]
fn import_json_updates_rejects_tree_create_target_not_matching_op_id() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(73)?;
    doc.get_tree("tree").create(None)?;
    doc.commit();

    let mut json = doc
        .export_json_updates_without_peer_compression(&VersionVector::default(), &doc.oplog_vv());
    for op in &mut json.changes[0].ops {
        if let JsonOpContent::Tree(JsonTreeOp::Create { target, .. }) = &mut op.content {
            *target = TreeID {
                peer: 73,
                counter: op.counter + 10,
            };
            break;
        }
    }

    let err = LoroDoc::new().import_json_updates(json).unwrap_err();
    assert!(
        err.to_string().contains("tree target"),
        "expected tree target validation error, got {err:?}"
    );

    Ok(())
}

#[test]
fn import_json_updates_rejects_nested_container_values() -> anyhow::Result<()> {
    fn nested_container_value(peer: u64, counter: i32) -> LoroValue {
        LoroValue::from(HashMap::from([(
            "nested".to_string(),
            LoroValue::Container(ContainerID::new_normal(
                ID::new(peer, counter),
                ContainerType::Map,
            )),
        )]))
    }

    let doc = LoroDoc::new();
    doc.set_peer_id(76)?;
    doc.get_map("root").insert("key", "value")?;
    doc.commit();

    let mut json = doc
        .export_json_updates_without_peer_compression(&VersionVector::default(), &doc.oplog_vv());
    for op in &mut json.changes[0].ops {
        if let JsonOpContent::Map(JsonMapOp::Insert { value, .. }) = &mut op.content {
            *value = nested_container_value(76, op.counter);
            break;
        }
    }

    let err = LoroDoc::new().import_json_updates(json).unwrap_err();
    assert!(
        err.to_string().contains("container"),
        "expected nested container validation error, got {err:?}"
    );

    let doc = LoroDoc::new();
    doc.set_peer_id(77)?;
    doc.get_list("list").insert(0, "value")?;
    doc.commit();

    let mut json = doc
        .export_json_updates_without_peer_compression(&VersionVector::default(), &doc.oplog_vv());
    for op in &mut json.changes[0].ops {
        if let JsonOpContent::List(JsonListOp::Insert { value, .. }) = &mut op.content {
            value[0] = nested_container_value(77, op.counter);
            break;
        }
    }

    let err = LoroDoc::new().import_json_updates(json).unwrap_err();
    assert!(
        err.to_string().contains("container"),
        "expected nested list container validation error, got {err:?}"
    );

    let doc = LoroDoc::new();
    doc.set_peer_id(78)?;
    let text = doc.get_text("text");
    text.insert(0, "a")?;
    text.mark(0..1, "bold", true)?;
    doc.commit();

    let mut json = doc
        .export_json_updates_without_peer_compression(&VersionVector::default(), &doc.oplog_vv());
    for op in &mut json.changes[0].ops {
        if let JsonOpContent::Text(JsonTextOp::Mark { style_value, .. }) = &mut op.content {
            *style_value = nested_container_value(78, op.counter);
            break;
        }
    }

    let err = LoroDoc::new().import_json_updates(json).unwrap_err();
    assert!(
        err.to_string().contains("container"),
        "expected nested text style container validation error, got {err:?}"
    );

    let doc = LoroDoc::new();
    doc.set_peer_id(79)?;
    let movable = doc.get_movable_list("movable");
    movable.insert(0, "seed")?;
    movable.set(0, "value")?;
    doc.commit();

    let mut json = doc
        .export_json_updates_without_peer_compression(&VersionVector::default(), &doc.oplog_vv());
    for op in &mut json.changes[0].ops {
        if let JsonOpContent::MovableList(JsonMovableListOp::Set { value, .. }) = &mut op.content {
            *value = nested_container_value(79, op.counter);
            break;
        }
    }

    let err = LoroDoc::new().import_json_updates(json).unwrap_err();
    assert!(
        err.to_string().contains("container"),
        "expected nested movable list container validation error, got {err:?}"
    );

    Ok(())
}
