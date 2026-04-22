use std::collections::BTreeSet;

use loro::{
    cursor::{PosType, Side},
    Container, ContainerTrait, ContainerType, ExpandType, ExportMode, LoroDoc, LoroList, LoroMap,
    LoroMovableList, LoroResult, LoroText, LoroValue, StyleConfig, StyleConfigMap, TextDelta,
    ToJson, ValueOrContainer,
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

fn container_name(container: &Container) -> &'static str {
    match container {
        Container::List(_) => "list",
        Container::Map(_) => "map",
        Container::Text(_) => "text",
        Container::Tree(_) => "tree",
        Container::MovableList(_) => "movable_list",
        Container::Unknown(_) => "unknown",
        #[cfg(feature = "counter")]
        Container::Counter(_) => "counter",
    }
}

fn value_or_container_name(value: &ValueOrContainer) -> String {
    match value {
        ValueOrContainer::Value(v) => format!("value:{:?}", v.to_json_value()),
        ValueOrContainer::Container(c) => {
            format!("container:{}:{:?}", container_name(c), c.id())
        }
    }
}

#[test]
fn map_contracts_cover_iteration_lookup_and_root_hiding() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;

    let map = doc.get_map("meta");
    map.insert("title", "draft")?;
    map.insert("count", 1)?;
    map.insert("alive", true)?;
    map.insert("none", LoroValue::Null)?;

    let body = map.insert_container("body", LoroText::new())?;
    body.insert(0, "hello")?;
    let items = map.get_or_create_container("items", LoroList::new())?;
    items.push("a")?;
    items.push("b")?;
    let nested_map = map.insert_container("details", LoroMap::new())?;
    nested_map.insert("kind", "nested")?;
    let nested_movable = map.insert_container("moves", LoroMovableList::new())?;
    nested_movable.push("x")?;

    doc.commit();

    doc.set_peer_id(2)?;
    map.insert("count", 2)?;
    map.delete("alive")?;
    nested_map.insert("version", 3)?;
    doc.commit();

    let keys = map
        .keys()
        .map(|key| key.to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        keys,
        BTreeSet::from([
            "body".to_string(),
            "count".to_string(),
            "details".to_string(),
            "items".to_string(),
            "moves".to_string(),
            "none".to_string(),
            "title".to_string(),
        ])
    );

    let values = map
        .values()
        .map(|value| value_or_container_name(&value))
        .collect::<BTreeSet<_>>();
    assert_eq!(values.len(), 7);
    assert!(values.iter().any(|entry| entry.starts_with("value:")));
    assert_eq!(map.get("none").unwrap().get_deep_value(), LoroValue::Null);
    assert!(values
        .iter()
        .any(|entry| entry.starts_with("container:text:")));
    assert!(values
        .iter()
        .any(|entry| entry.starts_with("container:list:")));
    assert!(values
        .iter()
        .any(|entry| entry.starts_with("container:movable_list:")));
    assert!(values
        .iter()
        .any(|entry| entry.starts_with("container:map:")));

    let mut seen = BTreeSet::new();
    map.for_each(|key, value| {
        seen.insert(format!("{key}={}", value_or_container_name(&value)));
    });
    assert!(seen.iter().any(|entry| entry.starts_with("title=value:")));
    assert!(seen.iter().any(|entry| entry.starts_with("count=value:")));
    assert!(seen.iter().any(|entry| entry.starts_with("none=value:")));
    assert!(seen
        .iter()
        .any(|entry| entry.starts_with("body=container:text:")));
    assert!(seen
        .iter()
        .any(|entry| entry.starts_with("items=container:list:")));

    assert_eq!(map.get_last_editor("title"), Some(1));
    assert_eq!(map.get_last_editor("count"), Some(2));
    assert_eq!(map.get_last_editor("body"), Some(1));
    assert_eq!(map.get_last_editor("details"), Some(1));
    assert_eq!(map.get_last_editor("moves"), Some(1));

    let body_container = doc.get_container(body.id()).expect("body should exist");
    assert_eq!(body_container.get_type(), ContainerType::Text);
    assert!(<LoroText as ContainerTrait>::try_from_container(body_container.clone()).is_some());
    assert!(<LoroMap as ContainerTrait>::try_from_container(body_container.clone()).is_none());

    let items_container = doc.get_container(items.id()).expect("items should exist");
    assert_eq!(items_container.get_type(), ContainerType::List);
    assert!(<LoroList as ContainerTrait>::try_from_container(items_container.clone()).is_some());
    assert!(<LoroText as ContainerTrait>::try_from_container(items_container.clone()).is_none());

    let details_container = doc
        .get_container(nested_map.id())
        .expect("details should exist");
    assert_eq!(details_container.get_type(), ContainerType::Map);
    assert!(<LoroMap as ContainerTrait>::try_from_container(details_container.clone()).is_some());
    assert!(
        <LoroMovableList as ContainerTrait>::try_from_container(details_container.clone())
            .is_none()
    );

    let moves_container = doc
        .get_container(nested_movable.id())
        .expect("moves should exist");
    assert_eq!(moves_container.get_type(), ContainerType::MovableList);
    assert!(
        <LoroMovableList as ContainerTrait>::try_from_container(moves_container.clone()).is_some()
    );
    assert!(<LoroText as ContainerTrait>::try_from_container(moves_container.clone()).is_none());

    assert!(doc.has_container(&map.id()));
    assert!(doc.has_container(&body.id()));
    assert!(doc.has_container(&items.id()));
    assert!(doc.has_container(&nested_map.id()));
    assert!(doc.has_container(&nested_movable.id()));

    doc.set_hide_empty_root_containers(true);
    map.clear()?;
    doc.delete_root_container(map.id());

    assert_eq!(deep_json(&doc), json!({}));
    assert!(doc.has_container(&map.id()));

    let snapshot = doc.export(ExportMode::Snapshot)?;
    let _restored = LoroDoc::from_snapshot(&snapshot)?;

    Ok(())
}

#[test]
fn list_contracts_cover_insert_delete_pop_clear_cursor_and_nested_containers() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(10)?;

    let list = doc.get_list("items");
    list.push("a")?;
    list.insert(1, "b")?;
    list.insert(1, "c")?;
    list.insert(3, "d")?;
    let nested_map = list.insert_container(1, LoroMap::new())?;
    nested_map.insert("kind", "nested")?;
    let nested_text = list.push_container(LoroText::new())?;
    nested_text.insert(0, "tail")?;
    doc.commit();

    assert_eq!(list.len(), 6);
    assert_eq!(list.get_id_at(0).map(|id| id.peer), Some(10));
    assert!(list.get_id_at(1).is_some());

    let cursor = list
        .get_cursor(2, Side::Middle)
        .expect("cursor should resolve");
    assert_eq!(
        doc.get_cursor_pos(&cursor)
            .expect("cursor should map")
            .current
            .pos,
        2
    );

    list.insert(0, "z")?;
    assert_eq!(
        doc.get_cursor_pos(&cursor)
            .expect("cursor should map")
            .current
            .pos,
        3
    );
    list.delete(0, 1)?;
    assert_eq!(
        doc.get_cursor_pos(&cursor)
            .expect("cursor should map")
            .current
            .pos,
        2
    );

    let vec = list.to_vec();
    assert_eq!(vec.len(), list.len());
    assert_eq!(vec[0], LoroValue::from("a"));
    assert_eq!(vec[2], LoroValue::from("c"));
    assert!(matches!(vec[1], LoroValue::Container(_)));
    assert!(matches!(vec[5], LoroValue::Container(_)));

    let mut iterated = Vec::new();
    list.for_each(|value| iterated.push(value_or_container_name(&value)));
    assert!(iterated.iter().any(|entry| entry.starts_with("value:")));
    assert!(iterated
        .iter()
        .any(|entry| entry.starts_with("container:map:")));
    assert!(iterated
        .iter()
        .any(|entry| entry.starts_with("container:text:")));

    let deep = list.get_deep_value().to_json_value();
    assert_eq!(
        deep,
        json!(["a", {"kind": "nested"}, "c", "b", "d", "tail"])
    );

    let removed = list.pop()?.expect("pop should return the last element");
    assert!(matches!(removed, LoroValue::Container(_)));
    assert_eq!(list.len(), 5);
    list.clear()?;
    assert!(list.is_empty());
    assert_eq!(list.get_value().to_json_value(), json!([]));

    Ok(())
}

#[test]
fn movable_list_contracts_cover_reorder_metadata_and_nested_containers() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(11)?;

    let list = doc.get_movable_list("ml");
    list.insert(0, "a")?;
    list.insert(1, "b")?;
    list.insert(2, "c")?;
    let nested_text = list.push_container(LoroText::new())?;
    nested_text.insert(0, "tail")?;
    doc.commit();

    let cursor = list
        .get_cursor(1, Side::Middle)
        .expect("cursor should resolve");
    assert_eq!(
        doc.get_cursor_pos(&cursor)
            .expect("cursor should map")
            .current
            .pos,
        1
    );
    assert_eq!(list.get_creator_at(0), Some(11));
    assert_eq!(list.get_last_mover_at(0), Some(11));
    assert_eq!(list.get_last_editor_at(0), Some(11));
    assert_eq!(list.get_creator_at(3), Some(11));

    doc.set_peer_id(12)?;
    list.mov(0, 2)?;
    assert_eq!(
        doc.get_cursor_pos(&cursor)
            .expect("cursor should map")
            .current
            .pos,
        0
    );
    assert_eq!(list.get_creator_at(2), Some(11));
    assert_eq!(list.get_last_mover_at(2), Some(12));

    doc.set_peer_id(13)?;
    list.set(1, "B")?;
    assert_eq!(list.get_last_editor_at(1), Some(13));

    let nested_map = list.set_container(3, LoroMap::new())?;
    nested_map.insert("kind", "nested")?;
    assert_eq!(list.get_creator_at(3), Some(11));
    assert_eq!(list.get_last_editor_at(3), Some(13));

    let deep = list.get_deep_value().to_json_value();
    assert_eq!(deep, json!(["b", "B", "a", {"kind": "nested"}]));

    let mut iterated = Vec::new();
    list.for_each(|value| iterated.push(value_or_container_name(&value)));
    assert!(iterated.iter().any(|entry| entry.starts_with("value:")));
    assert!(iterated
        .iter()
        .any(|entry| entry.starts_with("container:map:")));

    let vec = list.to_vec();
    assert_eq!(vec.len(), 4);
    assert!(matches!(vec[3], LoroValue::Container(_)));

    let removed = list.pop()?.expect("pop should remove the last item");
    assert!(matches!(removed, ValueOrContainer::Container(_)));
    list.delete(0, 1)?;
    assert_eq!(list.len(), 2);
    list.clear()?;
    assert!(list.is_empty());
    assert_eq!(list.get_value().to_json_value(), json!([]));

    Ok(())
}

#[test]
fn text_contracts_cover_unicode_positions_richtext_and_editor_info() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(21)?;

    let mut styles = StyleConfigMap::default_rich_text_config();
    styles.insert("bold".into(), StyleConfig::new().expand(ExpandType::None));
    doc.config_text_style(styles);

    let text = doc.get_text("text");
    text.insert(0, "A😀BC")?;
    doc.commit();

    let cursor = text
        .get_cursor(1, Side::Middle)
        .expect("text cursor should resolve");
    assert_eq!(
        doc.get_cursor_pos(&cursor)
            .expect("cursor should map")
            .current
            .pos,
        1
    );
    assert_eq!(text.get_editor_at_unicode_pos(0), Some(21));

    let mut chunks = Vec::new();
    text.iter(|chunk| {
        chunks.push(chunk.to_string());
        true
    });
    assert_eq!(chunks.concat(), text.to_string());
    assert_eq!(text.char_at(1)?, '😀');
    assert_eq!(text.slice(1, 3)?, "😀B");
    assert_eq!(
        text.convert_pos(1, PosType::Unicode, PosType::Bytes),
        Some(1)
    );
    assert_eq!(
        text.convert_pos(1, PosType::Unicode, PosType::Utf16),
        Some(1)
    );
    assert_eq!(
        text.convert_pos(5, PosType::Bytes, PosType::Unicode),
        Some(2)
    );
    assert_eq!(text.convert_pos(3, PosType::Utf16, PosType::Bytes), Some(5));

    text.mark(0..2, "bold", true)?;
    let delta = text.slice_delta(0, text.len_unicode(), PosType::Unicode)?;
    let expected_bold = LoroValue::from(true);
    assert!(delta.iter().any(|segment| match segment {
        TextDelta::Insert { insert, attributes } => {
            insert == "A😀"
                && attributes.as_ref().and_then(|attrs| attrs.get("bold")) == Some(&expected_bold)
        }
        _ => false,
    }));

    let removed = text.splice(1, 1, "Z")?;
    assert_eq!(removed, "😀");
    assert_eq!(text.to_string(), "AZBC");

    text.update("AZBC!", Default::default())
        .expect("update should succeed");
    assert_eq!(text.to_string(), "AZBC!");

    doc.set_peer_id(22)?;
    text.apply_delta(&[
        TextDelta::Retain {
            retain: 1,
            attributes: None,
        },
        TextDelta::Insert {
            insert: "-".into(),
            attributes: None,
        },
        TextDelta::Retain {
            retain: 4,
            attributes: None,
        },
    ])?;
    assert_eq!(text.to_string(), "A-ZBC!");
    assert_eq!(text.get_editor_at_unicode_pos(1), Some(22));
    assert!(text.get_richtext_value().to_json_value().is_array());

    Ok(())
}
