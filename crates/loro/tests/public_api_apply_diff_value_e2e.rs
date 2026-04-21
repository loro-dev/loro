use std::borrow::Cow;
use std::iter::FromIterator;

use loro::event::{Diff, DiffBatch, ListDiffItem, MapDelta};
use loro::{
    ApplyDiff, Container, ContainerID, ContainerType, Index, LoroDoc, LoroValue, TextDelta, ToJson,
    TreeParentId, ValueOrContainer, ID,
};
use rustc_hash::FxHashMap;
use serde_json::json;

fn text_insert(insert: &str, attributes: Option<FxHashMap<String, LoroValue>>) -> TextDelta {
    TextDelta::Insert {
        insert: insert.to_string(),
        attributes,
    }
}

fn text_retain(retain: usize) -> TextDelta {
    TextDelta::Retain {
        retain,
        attributes: None,
    }
}

#[test]
fn apply_diff_shallow_updates_strings_lists_and_maps() -> anyhow::Result<()> {
    let mut text = LoroValue::from("abcdef");
    let mut list = LoroValue::from(vec![1, 2, 3, 4]);
    let mut map = LoroValue::from(std::collections::HashMap::from([
        ("keep".to_string(), "old".into()),
        ("remove".to_string(), "gone".into()),
    ]));

    let text_diff = Diff::Text(vec![
        text_retain(2),
        TextDelta::Delete { delete: 2 },
        text_insert(
            "XY",
            Some(FxHashMap::from_iter([("bold".to_string(), true.into())])),
        ),
        text_retain(2),
    ]);
    text.apply_diff_shallow(&[text_diff.clone().into()]);
    assert_eq!(text.to_json_value(), json!("abXYef"));

    let list_diff = Diff::List(vec![
        ListDiffItem::Retain { retain: 1 },
        ListDiffItem::Delete { delete: 1 },
        ListDiffItem::Insert {
            insert: vec![
                ValueOrContainer::Value(9.into()),
                ValueOrContainer::Value(8.into()),
            ],
            is_move: false,
        },
        ListDiffItem::Retain { retain: 1 },
    ]);
    list.apply_diff_shallow(&[list_diff.clone().into()]);
    assert_eq!(list.to_json_value(), json!([1, 9, 8, 3, 4]));

    let map_diff = Diff::Map(MapDelta {
        updated: FxHashMap::from_iter([
            (
                Cow::Borrowed("keep"),
                Some(ValueOrContainer::Value("updated".into())),
            ),
            (
                Cow::Borrowed("added"),
                Some(ValueOrContainer::Value(42.into())),
            ),
            (Cow::Borrowed("remove"), None),
        ]),
    });
    map.apply_diff_shallow(&[map_diff.clone().into()]);
    assert_eq!(map.to_json_value(), json!({"keep": "updated", "added": 42}));

    Ok(())
}

#[test]
fn apply_diff_roundtrips_container_values_and_diff_batches() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let embedded_map = doc.get_map("embedded_map");
    embedded_map.insert("kind", "map")?;
    let embedded_text = doc.get_text("embedded_text");
    embedded_text.insert(0, "nested")?;
    doc.commit();

    let mut batch = DiffBatch::default();
    let text_cid = ContainerID::new_normal(ID::new(1, 1), ContainerType::Text);
    let list_cid = ContainerID::new_normal(ID::new(1, 2), ContainerType::List);
    let map_cid = ContainerID::new_normal(ID::new(1, 3), ContainerType::Map);

    let text_diff = Diff::Text(vec![
        text_retain(1),
        TextDelta::Delete { delete: 2 },
        text_insert("Z", None),
        text_retain(1),
    ]);
    let list_diff = Diff::List(vec![ListDiffItem::Insert {
        insert: vec![
            ValueOrContainer::Container(Container::Map(embedded_map.clone())),
            ValueOrContainer::Value(1.into()),
        ],
        is_move: false,
    }]);
    let map_diff = Diff::Map(MapDelta {
        updated: FxHashMap::from_iter([
            (
                Cow::Borrowed("nested"),
                Some(ValueOrContainer::Container(Container::Text(
                    embedded_text.clone(),
                ))),
            ),
            (
                Cow::Borrowed("keep"),
                Some(ValueOrContainer::Value(true.into())),
            ),
            (Cow::Borrowed("remove"), None),
        ]),
    });

    batch.push(text_cid.clone(), text_diff.clone()).unwrap();
    batch.push(list_cid, list_diff.clone()).unwrap();
    batch.push(map_cid, map_diff.clone()).unwrap();
    assert!(batch.push(text_cid, text_diff.clone()).is_err());
    assert_eq!(
        batch
            .iter()
            .map(|(cid, _)| cid.container_type())
            .collect::<Vec<_>>(),
        vec![ContainerType::Text, ContainerType::List, ContainerType::Map]
    );

    let mut text = LoroValue::from("abcd");
    text.apply_diff(&[text_diff.into()]);
    assert_eq!(text.to_json_value(), json!("aZd"));

    let mut list = LoroValue::from(Vec::<LoroValue>::new());
    list.apply_diff(&[list_diff.into()]);
    assert_eq!(list.to_json_value(), json!([{}, 1]));

    let mut map = LoroValue::from(std::collections::HashMap::<String, LoroValue>::new());
    map.apply_diff(&[map_diff.into()]);
    assert_eq!(map.to_json_value(), json!({"nested": "", "keep": true}));

    Ok(())
}

#[test]
fn apply_path_creates_nested_containers_and_updates_tree_nodes() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let embedded_map = doc.get_map("embedded_map");
    embedded_map.insert("kind", "map")?;
    let embedded_text = doc.get_text("embedded_text");
    embedded_text.insert(0, "nested")?;
    doc.commit();

    let mut root = LoroValue::from(std::collections::HashMap::<String, LoroValue>::new());
    let title_path = vec![Index::Key("doc".into()), Index::Key("title".into())].into();
    root.apply(
        &title_path,
        &[Diff::Text(vec![text_insert("Hello", None)]).into()],
    );

    let items_path = vec![Index::Key("doc".into()), Index::Key("items".into())].into();
    root.apply(
        &items_path,
        &[Diff::List(vec![ListDiffItem::Insert {
            insert: vec![ValueOrContainer::Container(Container::Map(
                embedded_map.clone(),
            ))],
            is_move: false,
        }])
        .into()],
    );

    let meta_path = vec![Index::Key("doc".into()), Index::Key("meta".into())].into();
    root.apply(
        &meta_path,
        &[Diff::Map(MapDelta {
            updated: FxHashMap::from_iter([(
                Cow::Borrowed("label"),
                Some(ValueOrContainer::Container(Container::Text(
                    embedded_text.clone(),
                ))),
            )]),
        })
        .into()],
    );

    assert_eq!(
        root.to_json_value(),
        json!({
            "doc": {
                "title": "Hello",
                "items": [{}],
                "meta": {"label": ""}
            }
        })
    );

    let tree = doc.get_tree("tree");
    let root_id = tree.create(TreeParentId::Root)?;
    tree.get_meta(root_id)?.insert("kind", "tree")?;
    doc.commit();
    let fractional_index = tree.fractional_index(root_id).unwrap();

    let mut tree_value = tree.get_value_with_meta();
    let node_path = vec![Index::Node(root_id)].into();
    tree_value.apply(
        &node_path,
        &[Diff::Map(MapDelta {
            updated: FxHashMap::from_iter([(
                Cow::Borrowed("flag"),
                Some(ValueOrContainer::Value(true.into())),
            )]),
        })
        .into()],
    );

    assert_eq!(
        tree_value.to_json_value(),
        json!([{
            "id": root_id.to_string(),
            "parent": null,
            "meta": {"kind": "tree", "flag": true},
            "index": 0,
            "children": [],
            "fractional_index": fractional_index
        }])
    );

    Ok(())
}
