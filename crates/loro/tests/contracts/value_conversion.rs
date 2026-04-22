use std::{borrow::Cow, collections::HashMap, iter::FromIterator};

use loro::{
    event::{Diff, ListDiffItem, MapDelta},
    ApplyDiff, ContainerTrait, ContainerType, Index, JsonSchema, LoroDoc, LoroList, LoroMap,
    LoroText, LoroValue, TextDelta, ToJson, ValueOrContainer, VersionVector,
};
use pretty_assertions::assert_eq;
use rustc_hash::FxHashMap;
use serde_json::json;

fn text_insert(insert: &str) -> TextDelta {
    TextDelta::Insert {
        insert: insert.to_string(),
        attributes: None,
    }
}

fn text_retain(retain: usize) -> TextDelta {
    TextDelta::Retain {
        retain,
        attributes: None,
    }
}

fn assert_to_json_roundtrip(value: &LoroValue) {
    let compact = value.to_json();
    let pretty = value.to_json_pretty();

    assert_eq!(LoroValue::from_json(&compact), value.clone());
    assert_eq!(LoroValue::from_json(&pretty), value.clone());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&compact).unwrap(),
        value.to_json_value()
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&pretty).unwrap(),
        value.to_json_value()
    );
}

#[test]
fn loro_value_defaults_and_to_json_roundtrip_cover_contracts() -> anyhow::Result<()> {
    assert_eq!(LoroValue::default(), LoroValue::Null);
    assert_eq!(
        ContainerType::Map.default_value(),
        LoroValue::from(HashMap::<String, LoroValue>::new())
    );
    assert_eq!(
        ContainerType::List.default_value(),
        LoroValue::from(Vec::<LoroValue>::new())
    );
    assert_eq!(
        ContainerType::Text.default_value(),
        LoroValue::from(String::new())
    );
    assert_eq!(
        ContainerType::Tree.default_value(),
        LoroValue::from(Vec::<LoroValue>::new())
    );
    assert_eq!(
        ContainerType::MovableList.default_value(),
        LoroValue::from(Vec::<LoroValue>::new())
    );
    #[cfg(feature = "counter")]
    assert_eq!(ContainerType::Counter.default_value(), LoroValue::from(0.0));

    let doc = LoroDoc::new();
    let container = doc
        .get_map("root")
        .insert_container("child", LoroText::new())?;

    let binary = LoroValue::from(vec![1_u8, 2, 3, 255]);
    assert_eq!(binary.to_json_value(), json!([1, 2, 3, 255]));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&binary.to_json())?,
        json!([1, 2, 3, 255])
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&binary.to_json_pretty())?,
        json!([1, 2, 3, 255])
    );

    let values = vec![
        LoroValue::Null,
        false.into(),
        true.into(),
        123_i64.into(),
        (-12.5_f64).into(),
        "hello".into(),
        LoroValue::from(vec![
            LoroValue::Null,
            true.into(),
            7_i64.into(),
            3.25_f64.into(),
            "text".into(),
            LoroValue::from(vec![4_i64, 5_i64]),
            LoroValue::from(HashMap::from([
                ("nested".to_string(), LoroValue::from(vec![1_i64, 2_i64])),
                ("flag".to_string(), true.into()),
            ])),
        ]),
        LoroValue::from(HashMap::from([
            ("null".to_string(), LoroValue::Null),
            ("count".to_string(), 9_i64.into()),
            ("ratio".to_string(), 1.25_f64.into()),
            ("label".to_string(), "map".into()),
            ("items".to_string(), LoroValue::from(vec![1_i64, 2_i64])),
        ])),
        LoroValue::from(container.id()),
    ];

    for value in values {
        assert_to_json_roundtrip(&value);
    }

    Ok(())
}

#[test]
fn apply_diff_handles_nested_paths_existing_list_indexes_and_missing_tree_nodes(
) -> anyhow::Result<()> {
    let mut root = LoroValue::from(HashMap::<String, LoroValue>::new());

    root.apply(
        &vec![Index::Key("doc".into()), Index::Key("title".into())].into(),
        &[Diff::Text(vec![text_insert("Hello")]).into()],
    );

    root.apply(
        &vec![Index::Key("doc".into()), Index::Key("meta".into())].into(),
        &[Diff::Map(MapDelta {
            updated: FxHashMap::from_iter([
                (
                    Cow::Borrowed("author"),
                    Some(ValueOrContainer::Value("Ada".into())),
                ),
                (
                    Cow::Borrowed("pages"),
                    Some(ValueOrContainer::Value(3_i64.into())),
                ),
                (
                    Cow::Borrowed("tags"),
                    Some(ValueOrContainer::Value(LoroValue::from(vec![
                        LoroValue::from("rust"),
                        LoroValue::from("crdt"),
                    ]))),
                ),
            ]),
        })
        .into()],
    );

    root.apply(
        &vec![Index::Key("doc".into()), Index::Key("items".into())].into(),
        &[Diff::List(vec![ListDiffItem::Insert {
            insert: vec![
                ValueOrContainer::Value("alpha".into()),
                ValueOrContainer::Value("beta".into()),
                ValueOrContainer::Value(LoroValue::from(vec![1_i64, 2_i64])),
            ],
            is_move: false,
        }])
        .into()],
    );

    root.apply(
        &vec![
            Index::Key("doc".into()),
            Index::Key("items".into()),
            Index::Seq(1),
        ]
        .into(),
        &[Diff::Text(vec![
            text_retain(2),
            TextDelta::Delete { delete: 2 },
            text_insert("TA"),
        ])
        .into()],
    );

    assert_eq!(
        root.to_json_value(),
        json!({
            "doc": {
                "title": "Hello",
                "meta": {
                    "author": "Ada",
                    "pages": 3,
                    "tags": ["rust", "crdt"],
                },
                "items": ["alpha", "beTA", [1, 2]],
            }
        })
    );

    let doc = LoroDoc::new();
    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let node = tree.create(None)?;
    tree.get_meta(node)?.insert("kind", "live")?;
    doc.commit();

    tree.delete(node)?;
    doc.commit();

    let mut tree_value = tree.get_value_with_meta();
    let before = tree_value.clone();
    tree_value.apply(
        &vec![Index::Node(node)].into(),
        &[Diff::Map(MapDelta {
            updated: FxHashMap::from_iter([(
                Cow::Borrowed("ghost"),
                Some(ValueOrContainer::Value(true.into())),
            )]),
        })
        .into()],
    );
    assert_eq!(tree_value, before);

    Ok(())
}

#[test]
fn json_schema_roundtrip_keeps_value_types_and_imports_back_state() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(17)?;

    let root = doc.get_map("root");
    root.insert("null", LoroValue::Null)?;
    root.insert("bool", true)?;
    root.insert("i64", 42_i64)?;
    root.insert("double", 3.5_f64)?;
    root.insert("string", "hello")?;
    root.insert("binary", vec![1_u8, 2, 3, 255])?;
    root.insert(
        "list_value",
        LoroValue::from(vec![
            LoroValue::Null,
            true.into(),
            7_i64.into(),
            1.25_f64.into(),
            "leaf".into(),
            vec![4_u8, 5].into(),
            LoroValue::from(HashMap::from([
                ("nested".to_string(), "value".into()),
                ("count".to_string(), 9_i64.into()),
            ])),
        ]),
    )?;
    root.insert(
        "map_value",
        LoroValue::from(HashMap::from([
            ("nested".to_string(), LoroValue::from(vec![1_i64, 2_i64])),
            ("flag".to_string(), true.into()),
        ])),
    )?;

    let child_map = root.insert_container("child_map", LoroMap::new())?;
    child_map.insert("kind", "map")?;
    child_map.insert("payload", vec![9_u8, 8, 7])?;

    let child_list = root.insert_container("child_list", LoroList::new())?;
    child_list.push("alpha")?;
    child_list.push(LoroValue::from(HashMap::from([
        ("flag".to_string(), true.into()),
        ("label".to_string(), "list-item".into()),
    ])))?;
    let nested_text = child_list.push_container(LoroText::new())?;
    nested_text.insert(0, "nested")?;
    nested_text.mark(0..6, "bold", true)?;

    let body = root.insert_container("body", LoroText::new())?;
    body.insert(0, "hello")?;
    body.mark(0..5, "italic", true)?;

    #[cfg(feature = "counter")]
    {
        let counter = root.insert_container("counter", loro::LoroCounter::new())?;
        counter.increment(5.5)?;
        counter.decrement(1.0)?;
    }

    doc.commit();

    let start = VersionVector::default();
    let end = doc.oplog_vv();
    let compressed = doc.export_json_updates(&start, &end);
    let uncompressed = doc.export_json_updates_without_peer_compression(&start, &end);

    let compressed_json = serde_json::to_string(&compressed)?;
    let uncompressed_json = serde_json::to_string(&uncompressed)?;

    let compressed_from_serde: JsonSchema = serde_json::from_str(&compressed_json)?;
    let compressed_from_try_into: JsonSchema = compressed_json.as_str().try_into()?;
    let uncompressed_from_serde: JsonSchema = serde_json::from_str(&uncompressed_json)?;
    let uncompressed_from_try_into: JsonSchema = uncompressed_json.as_str().try_into()?;

    assert_eq!(
        serde_json::to_value(&compressed_from_serde)?,
        serde_json::to_value(&compressed)?
    );
    assert_eq!(
        serde_json::to_value(&compressed_from_try_into)?,
        serde_json::to_value(&compressed)?
    );
    assert_eq!(
        serde_json::to_value(&uncompressed_from_serde)?,
        serde_json::to_value(&uncompressed)?
    );
    assert_eq!(
        serde_json::to_value(&uncompressed_from_try_into)?,
        serde_json::to_value(&uncompressed)?
    );

    let imported_from_compressed = LoroDoc::new();
    imported_from_compressed.import_json_updates(compressed_from_try_into.clone())?;
    assert_eq!(
        imported_from_compressed.get_deep_value().to_json_value(),
        doc.get_deep_value().to_json_value()
    );

    let imported_from_compressed_json = LoroDoc::new();
    imported_from_compressed_json.import_json_updates(compressed_json.clone())?;
    assert_eq!(
        imported_from_compressed_json
            .get_deep_value()
            .to_json_value(),
        doc.get_deep_value().to_json_value()
    );

    let imported_from_uncompressed = LoroDoc::new();
    imported_from_uncompressed.import_json_updates(uncompressed_from_serde.clone())?;
    assert_eq!(
        imported_from_uncompressed.get_deep_value().to_json_value(),
        doc.get_deep_value().to_json_value()
    );

    let imported_from_uncompressed_json = LoroDoc::new();
    imported_from_uncompressed_json.import_json_updates(uncompressed_json.clone())?;
    assert_eq!(
        imported_from_uncompressed_json
            .get_deep_value()
            .to_json_value(),
        doc.get_deep_value().to_json_value()
    );

    Ok(())
}
