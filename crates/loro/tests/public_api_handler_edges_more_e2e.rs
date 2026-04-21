use std::collections::BTreeSet;

use loro::{
    cursor::{PosType, Side},
    Container, ContainerTrait, ExpandType, LoroDoc, LoroError, LoroList, LoroMap, LoroMovableList,
    LoroResult, LoroText, LoroTree, StyleConfig, StyleConfigMap, TextDelta, ToJson, TreeParentId,
    ValueOrContainer,
};
use pretty_assertions::assert_eq;
use serde_json::json;

#[cfg(feature = "counter")]
use loro::LoroCounter;

fn expect_text(value: ValueOrContainer) -> LoroText {
    match value {
        ValueOrContainer::Container(Container::Text(text)) => text,
        other => panic!("expected text container, found {other:?}"),
    }
}

fn expect_list(value: ValueOrContainer) -> LoroList {
    match value {
        ValueOrContainer::Container(Container::List(list)) => list,
        other => panic!("expected list container, found {other:?}"),
    }
}

fn expect_movable_list(value: ValueOrContainer) -> LoroMovableList {
    match value {
        ValueOrContainer::Container(Container::MovableList(list)) => list,
        other => panic!("expected movable list container, found {other:?}"),
    }
}

fn expect_tree(value: ValueOrContainer) -> LoroTree {
    match value {
        ValueOrContainer::Container(Container::Tree(tree)) => tree,
        other => panic!("expected tree container, found {other:?}"),
    }
}

fn value_kind(value: &ValueOrContainer) -> String {
    match value {
        ValueOrContainer::Value(v) => format!("value:{:?}", v.to_json_value()),
        ValueOrContainer::Container(container) => match container {
            Container::List(_) => "container:list".to_string(),
            Container::Map(_) => "container:map".to_string(),
            Container::Text(_) => "container:text".to_string(),
            Container::Tree(_) => "container:tree".to_string(),
            Container::MovableList(_) => "container:movable_list".to_string(),
            #[cfg(feature = "counter")]
            Container::Counter(_) => "container:counter".to_string(),
            Container::Unknown(_) => "container:unknown".to_string(),
        },
    }
}

fn assert_container_deleted<T>(result: LoroResult<T>) {
    match result {
        Err(LoroError::ContainerDeleted { .. }) => {}
        _ => panic!("expected ContainerDeleted error"),
    }
}

#[test]
fn detached_bundle_contracts_cover_child_handler_lookup_attachment_and_deletion() -> LoroResult<()>
{
    let doc = LoroDoc::new();
    doc.set_peer_id(11)?;
    let root = doc.get_map("root");

    let bundle = LoroMap::new();
    let bundle_probe = bundle.clone();

    assert!(!bundle.is_attached());
    assert!(bundle.doc().is_none());
    assert!(bundle.get_attached().is_none());
    assert!(bundle.is_empty());
    assert_eq!(bundle.len(), 0);

    bundle.insert("title", "draft")?;
    bundle.insert("count", 1)?;
    bundle.insert("active", true)?;

    let text = bundle.insert_container("text", LoroText::new())?;
    text.insert(0, "hello")?;

    let list = bundle.insert_container("list", LoroList::new())?;
    list.push("a")?;
    list.push("b")?;
    let list_meta = list.insert_container(1, LoroMap::new())?;
    list_meta.insert("kind", "nested")?;

    let moves = bundle.insert_container("moves", LoroMovableList::new())?;
    moves.push("x")?;
    moves.push("y")?;
    moves.mov(0, 1)?;

    let tree = bundle.insert_container("tree", LoroTree::new())?;
    tree.enable_fractional_index(0);
    let tree_root = tree.create(TreeParentId::Root)?;
    let tree_child = tree.create_at(tree_root, 0)?;
    tree.get_meta(tree_root)?.insert("title", "root")?;
    tree.get_meta(tree_child)?.insert("title", "child")?;

    #[cfg(feature = "counter")]
    let counter = bundle.insert_container("counter", LoroCounter::new())?;
    #[cfg(feature = "counter")]
    {
        counter.increment(2.5)?;
        counter.decrement(0.5)?;
    }

    assert_eq!(tree.nodes().len(), 2);
    assert_eq!(tree.children(TreeParentId::Root), Some(vec![tree_root]));
    assert_eq!(tree.children(tree_root), Some(vec![tree_child]));
    assert_eq!(tree.parent(tree_child), Some(TreeParentId::Node(tree_root)));
    assert_eq!(
        tree.get_meta(tree_child)?
            .get("title")
            .unwrap()
            .get_deep_value(),
        "child".into()
    );

    let keys = bundle
        .keys()
        .map(|key| key.to_string())
        .collect::<BTreeSet<_>>();
    let mut expected_keys = BTreeSet::from([
        "active".to_string(),
        "count".to_string(),
        "list".to_string(),
        "moves".to_string(),
        "text".to_string(),
        "title".to_string(),
        "tree".to_string(),
    ]);
    #[cfg(feature = "counter")]
    {
        expected_keys.insert("counter".to_string());
    }
    assert_eq!(keys, expected_keys);

    let values = bundle
        .values()
        .map(|value| value_kind(&value))
        .collect::<BTreeSet<_>>();
    assert!(values.iter().any(|entry| entry.starts_with("value:")));
    assert!(values.iter().any(|entry| entry == "container:text"));
    assert!(values.iter().any(|entry| entry == "container:list"));
    assert!(values.iter().any(|entry| entry == "container:movable_list"));
    assert!(values.iter().any(|entry| entry == "container:tree"));
    #[cfg(feature = "counter")]
    assert!(values.iter().any(|entry| entry == "container:counter"));

    let mut seen = BTreeSet::new();
    bundle.for_each(|key, value| {
        seen.insert(format!("{key}={}", value_kind(&value)));
    });
    assert!(seen.iter().any(|entry| entry.starts_with("title=value:")));
    assert!(seen
        .iter()
        .any(|entry| entry.starts_with("text=container:text")));
    assert!(seen
        .iter()
        .any(|entry| entry.starts_with("list=container:list")));
    assert!(seen
        .iter()
        .any(|entry| entry.starts_with("moves=container:movable_list")));

    let attached_bundle = root.insert_container("bundle", bundle)?;
    doc.commit();

    assert!(attached_bundle.is_attached());
    assert!(attached_bundle.doc().is_some());
    assert!(bundle_probe.get_attached().is_some());
    assert!(!bundle_probe.is_attached());

    let attached_handler = attached_bundle.to_handler();
    let attached_text = Container::from_handler(attached_handler.get_child_handler("text")?);
    assert!(matches!(attached_text, Container::Text(_)));
    let attached_list = Container::from_handler(attached_handler.get_child_handler("list")?);
    assert!(matches!(attached_list, Container::List(_)));

    let attached_text = expect_text(attached_bundle.get("text").unwrap())
        .get_attached()
        .unwrap();
    let attached_list = expect_list(attached_bundle.get("list").unwrap())
        .get_attached()
        .unwrap();
    let attached_moves = expect_movable_list(attached_bundle.get("moves").unwrap())
        .get_attached()
        .unwrap();
    let attached_tree = expect_tree(attached_bundle.get("tree").unwrap())
        .get_attached()
        .unwrap();
    #[cfg(feature = "counter")]
    let attached_counter = match attached_bundle.get("counter").unwrap() {
        ValueOrContainer::Container(Container::Counter(counter)) => counter
            .get_attached()
            .expect("counter should have an attached clone"),
        other => panic!("expected counter container, found {other:?}"),
    };

    assert!(attached_text.is_attached());
    assert!(attached_list.is_attached());
    assert!(attached_moves.is_attached());
    assert!(attached_tree.is_attached());
    #[cfg(feature = "counter")]
    assert!(attached_counter.is_attached());
    assert_eq!(attached_text.to_string(), "hello");
    assert_eq!(
        attached_list.get_deep_value().to_json_value(),
        json!(["a", {"kind": "nested"}, "b"])
    );
    assert_eq!(
        attached_moves.get_deep_value().to_json_value(),
        json!(["y", "x"])
    );
    assert_eq!(attached_tree.nodes().len(), 2);
    let attached_roots = attached_tree.roots();
    assert_eq!(attached_roots.len(), 1);
    let attached_root = attached_roots[0];
    let attached_children = attached_tree.children(attached_root).unwrap();
    assert_eq!(attached_children.len(), 1);
    let attached_child = attached_children[0];
    assert_eq!(
        attached_tree.parent(attached_child),
        Some(TreeParentId::Node(attached_root))
    );
    assert_eq!(
        attached_tree
            .get_meta(attached_child)?
            .get("title")
            .unwrap()
            .get_deep_value(),
        "child".into()
    );

    let probe_text = expect_text(bundle_probe.get("text").unwrap());
    let probe_list = expect_list(bundle_probe.get("list").unwrap());
    let probe_moves = expect_movable_list(bundle_probe.get("moves").unwrap());
    let probe_tree = expect_tree(bundle_probe.get("tree").unwrap());
    assert!(!probe_text.is_attached());
    assert!(!probe_list.is_attached());
    assert!(!probe_moves.is_attached());
    assert!(!probe_tree.is_attached());
    assert!(probe_text.get_attached().is_some());
    assert!(probe_list.get_attached().is_some());
    assert!(probe_moves.get_attached().is_some());
    assert!(probe_tree.get_attached().is_some());

    root.delete("bundle")?;
    assert!(attached_bundle.is_deleted());
    assert!(attached_text.is_deleted());
    assert!(attached_list.is_deleted());
    assert!(attached_moves.is_deleted());
    assert!(attached_tree.is_deleted());
    #[cfg(feature = "counter")]
    assert!(attached_counter.is_deleted());
    assert_container_deleted(attached_bundle.insert("after_delete", "x"));
    assert_container_deleted(attached_text.insert(0, "!"));
    assert_container_deleted(attached_list.push("tail"));
    assert_container_deleted(attached_moves.push("tail"));
    assert_container_deleted(attached_tree.create(TreeParentId::Root));

    #[cfg(feature = "counter")]
    {
        assert_eq!(attached_counter.get(), 2.0);
        assert!(attached_counter.doc().is_some());
        assert_container_deleted(attached_counter.increment(1.0));
    }

    Ok(())
}

#[test]
fn attached_handlers_cover_bounds_get_child_handler_and_deleted_children() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(12)?;

    let map = doc.get_map("meta");
    map.insert("title", "draft")?;
    let map_text = map.insert_container("text", LoroText::new())?;
    map_text.insert(0, "hello")?;
    let map_list = map.insert_container("list", LoroList::new())?;
    map_list.push("a")?;
    map_list.push("b")?;
    let map_nested = map.insert_container("nested", LoroMap::new())?;
    map_nested.insert("kind", "nested")?;

    let list = doc.get_list("items");
    list.push("x")?;
    list.push("y")?;
    let list_nested = list.insert_container(1, LoroMap::new())?;
    list_nested.insert("name", "child")?;

    let movable = doc.get_movable_list("moves");
    movable.insert(0, "m0")?;
    movable.insert(1, "m1")?;
    let movable_text = movable.set_container(1, LoroText::new())?;
    movable_text.insert(0, "tail")?;

    doc.commit();

    let map_child = Container::from_handler(map.to_handler().get_child_handler("text")?);
    assert!(matches!(map_child, Container::Text(_)));
    assert_eq!(map_child.id(), map_text.id());

    let list_child = Container::from_handler(list.to_handler().get_child_handler(1)?);
    assert!(matches!(list_child, Container::Map(_)));
    assert_eq!(list_child.id(), list_nested.id());

    assert_eq!(map.get("title").unwrap().get_deep_value(), "draft".into());
    assert!(matches!(
        map.get("text").unwrap(),
        ValueOrContainer::Container(_)
    ));
    assert!(matches!(
        list.get(1).unwrap(),
        ValueOrContainer::Container(_)
    ));
    assert!(matches!(
        movable.get(1).unwrap(),
        ValueOrContainer::Container(_)
    ));

    let map_values = map
        .values()
        .map(|value| value_kind(&value))
        .collect::<BTreeSet<_>>();
    assert!(map_values.iter().any(|entry| entry == "container:text"));
    assert!(map_values.iter().any(|entry| entry == "container:list"));
    assert!(map_values.iter().any(|entry| entry == "container:map"));

    let mut list_values = Vec::new();
    list.for_each(|value| list_values.push(value_kind(&value)));
    assert!(list_values.iter().any(|entry| entry == "container:map"));
    assert!(list_values.iter().any(|entry| entry.starts_with("value:")));

    assert!(list.len() >= 2);
    assert!(movable.len() >= 2);
    assert!(!list.is_empty());
    assert!(!movable.is_empty());
    assert!(list.get_cursor(1, Side::Middle).is_some());

    assert!(list.insert(5, "boom").is_err());
    assert!(list.delete(5, 1).is_err());
    assert!(movable.insert(9, "boom").is_err());
    assert!(movable.set(9, "boom").is_err());
    assert!(movable.delete(9, 1).is_err());
    assert!(movable.mov(0, 9).is_err());
    assert!(map_text.insert(6, "!").is_err());
    assert!(map_text.delete(6, 1).is_err());
    assert!(map_text.splice(6, 1, "!").is_err());

    let cursor = map_text
        .get_cursor(1, Side::Middle)
        .expect("cursor should exist");
    assert_eq!(
        doc.get_cursor_pos(&cursor)
            .expect("cursor should map")
            .current
            .pos,
        1
    );
    assert_eq!(map_text.get_editor_at_unicode_pos(0), Some(12));
    assert_eq!(movable.get_creator_at(0), Some(12));
    assert_eq!(movable.get_last_editor_at(0), Some(12));
    assert_eq!(movable.get_last_mover_at(0), Some(12));

    map.delete("nested")?;
    list.delete(1, 1)?;
    movable.delete(1, 1)?;
    assert!(map_nested.is_deleted());
    assert!(list_nested.is_deleted());
    assert!(movable_text.is_deleted());
    assert_container_deleted(map_nested.insert("after", 1));
    assert_container_deleted(list_nested.insert("after", 1));
    assert_container_deleted(movable_text.insert(0, "after"));

    map.clear()?;
    list.clear()?;
    movable.clear()?;
    assert!(map.is_empty());
    assert!(list.is_empty());
    assert!(movable.is_empty());

    Ok(())
}

#[test]
fn text_contracts_cover_positions_deltas_update_by_line_apply_delta_and_clear() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(13)?;

    let mut styles = StyleConfigMap::default_rich_text_config();
    styles.insert("bold".into(), StyleConfig::new().expand(ExpandType::None));
    doc.config_text_style(styles);

    let text = doc.get_text("text");
    assert!(text.is_empty());
    assert_eq!(text.len_utf8(), 0);
    assert_eq!(text.len_utf16(), 0);
    assert_eq!(text.len_unicode(), 0);

    text.insert(0, "A😀BC\nline2")?;
    doc.commit();

    assert!(!text.is_empty());
    assert_eq!(text.len_utf8(), 13);
    assert_eq!(text.len_utf16(), 11);
    assert_eq!(text.len_unicode(), 10);
    assert_eq!(text.get_editor_at_unicode_pos(0), Some(13));
    assert_eq!(text.char_at(1)?, '😀');
    assert_eq!(text.slice(1, 4)?, "😀BC");
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

    let cursor = text
        .get_cursor(1, Side::Middle)
        .expect("cursor should exist");
    assert_eq!(
        doc.get_cursor_pos(&cursor)
            .expect("cursor should map")
            .current
            .pos,
        1
    );

    text.mark(0..2, "bold", true)?;
    let delta = text.slice_delta(1, 3, PosType::Unicode)?;
    assert_eq!(delta.len(), 2);
    match &delta[0] {
        TextDelta::Insert { insert, attributes } => {
            assert_eq!(insert, "😀");
            assert_eq!(
                attributes.as_ref().and_then(|attrs| attrs.get("bold")),
                Some(&true.into())
            );
        }
        other => panic!("expected emoji insert, got {other:?}"),
    }
    match &delta[1] {
        TextDelta::Insert { insert, attributes } => {
            assert_eq!(insert, "B");
            assert!(attributes.is_none());
        }
        other => panic!("expected B insert, got {other:?}"),
    }
    text.unmark(0..2, "bold")?;

    let mut chunks = Vec::new();
    text.iter(|chunk| {
        chunks.push(chunk.to_string());
        true
    });
    assert_eq!(chunks.concat(), text.to_string());

    text.update_by_line("A😀ZBC\nline3", Default::default())
        .expect("update_by_line should succeed");
    assert_eq!(text.to_string(), "A😀ZBC\nline3");

    let removed = text.splice(2, 1, "!")?;
    assert_eq!(removed, "Z");
    assert_eq!(text.to_string(), "A😀!BC\nline3");

    let patch = doc.get_text("patch");
    patch.insert(0, "ABC")?;
    patch.apply_delta(&[
        TextDelta::Retain {
            retain: 1,
            attributes: None,
        },
        TextDelta::Delete { delete: 1 },
        TextDelta::Insert {
            insert: "-".into(),
            attributes: None,
        },
        TextDelta::Retain {
            retain: 1,
            attributes: None,
        },
    ])?;
    assert_eq!(patch.to_string(), "A-C");

    text.delete(0, text.len_unicode())?;
    assert_eq!(text.to_string(), "");
    assert_eq!(text.len_utf8(), 0);
    assert_eq!(text.len_utf16(), 0);
    assert_eq!(text.len_unicode(), 0);
    assert!(text.get_richtext_value().to_json_value().is_array());
    assert!(text.delete(0, 1).is_err());

    Ok(())
}

#[cfg(feature = "counter")]
#[test]
fn counter_contracts_cover_attachment_and_deletion() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(14)?;

    let root = doc.get_map("root");
    let counter = LoroCounter::new();

    assert!(!counter.is_attached());
    assert!(!counter.is_deleted());
    assert!(counter.doc().is_none());
    assert_eq!(counter.get(), 0.0);

    counter.increment(1.25)?;
    counter.decrement(0.25)?;
    assert_eq!(counter.get_value(), 1.0);

    let attached_counter = root.insert_container("counter", counter.clone())?;
    assert!(attached_counter.is_attached());
    assert!(counter.get_attached().is_some());
    assert_eq!(attached_counter.get(), 1.0);

    doc.commit();
    assert_eq!(
        root.get("counter")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!(1.0)
    );

    root.delete("counter")?;
    assert!(attached_counter.is_deleted());
    assert_container_deleted(attached_counter.increment(1.0));
    assert_container_deleted(attached_counter.decrement(1.0));

    Ok(())
}
