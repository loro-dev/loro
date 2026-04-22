use loro::{
    cursor::Side, ContainerID, ContainerTrait, LoroDoc, LoroList, LoroMap, LoroMovableList,
    LoroResult, LoroText, LoroValue, ToJson,
};
use pretty_assertions::assert_eq;
use serde_json::json;

fn raw_container_value(id: ContainerID) -> LoroValue {
    LoroValue::Container(id)
}

#[test]
fn list_map_and_movable_list_reject_raw_container_values() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(501)?;

    let root = doc.get_map("root");
    let text = root.insert_container("text", LoroText::new())?;
    text.insert(0, "child")?;

    let list = doc.get_list("list");
    assert!(list.insert(0, raw_container_value(text.id())).is_err());
    list.push("plain")?;
    assert_eq!(list.get_deep_value().to_json_value(), json!(["plain"]));

    let movable = doc.get_movable_list("movable");
    assert!(movable.insert(0, raw_container_value(text.id())).is_err());
    movable.push("plain")?;
    assert_eq!(movable.get_deep_value().to_json_value(), json!(["plain"]));

    let map = doc.get_map("map");
    assert!(map.insert("raw", raw_container_value(text.id())).is_err());
    map.insert("plain", "value")?;
    assert_eq!(
        map.get_deep_value().to_json_value(),
        json!({"plain": "value"})
    );

    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "root": { "text": "child" },
            "list": ["plain"],
            "movable": ["plain"],
            "map": { "plain": "value" }
        })
    );

    Ok(())
}

#[test]
fn list_and_movable_list_empty_and_boundary_operations_are_stable() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(502)?;

    let list = doc.get_list("list");
    assert_eq!(list.pop()?, None);
    list.delete(0, 0)?;
    let empty_list_cursor = list
        .get_cursor(0, Side::Middle)
        .expect("empty attached list should have a boundary cursor");
    assert_eq!(empty_list_cursor.id, None);
    assert_eq!(empty_list_cursor.side, Side::Left);
    assert_eq!(
        doc.get_cursor_pos(&empty_list_cursor)
            .expect("empty list cursor should resolve")
            .current
            .pos,
        0
    );
    assert!(list.insert(1, "out").is_err());
    assert!(list.delete(0, 1).is_err());

    list.push("a")?;
    list.push("b")?;
    let right_cursor = list
        .get_cursor(list.len(), Side::Middle)
        .expect("right boundary cursor should be available");
    assert_eq!(right_cursor.id, None);
    assert_eq!(right_cursor.side, Side::Right);
    assert_eq!(
        doc.get_cursor_pos(&right_cursor)
            .expect("right boundary cursor should resolve")
            .current
            .pos,
        2
    );
    assert_eq!(list.pop()?, Some("b".into()));
    assert_eq!(list.get_deep_value().to_json_value(), json!(["a"]));

    let movable = doc.get_movable_list("movable");
    assert!(movable.pop()?.is_none());
    movable.delete(0, 0)?;
    let empty_movable_cursor = movable
        .get_cursor(0, Side::Middle)
        .expect("empty attached movable list should have a boundary cursor");
    assert_eq!(empty_movable_cursor.id, None);
    assert_eq!(empty_movable_cursor.side, Side::Left);
    assert_eq!(
        doc.get_cursor_pos(&empty_movable_cursor)
            .expect("empty movable-list cursor should resolve")
            .current
            .pos,
        0
    );
    assert!(movable.insert(1, "out").is_err());
    movable.mov(0, 0)?;
    assert!(movable.mov(0, 1).is_err());
    assert!(movable.delete(0, 1).is_err());

    movable.push("a")?;
    movable.push("b")?;
    movable.mov(0, 1)?;
    assert_eq!(movable.get_deep_value().to_json_value(), json!(["b", "a"]));
    assert_eq!(
        movable
            .pop()?
            .expect("last item should exist")
            .get_deep_value(),
        "a".into()
    );
    assert_eq!(movable.get_deep_value().to_json_value(), json!(["b"]));

    Ok(())
}

#[test]
fn detached_list_and_movable_list_boundary_contracts_match_attached_where_possible(
) -> LoroResult<()> {
    let list = LoroList::new();
    assert_eq!(list.pop()?, None);
    list.delete(0, 0)?;
    assert!(list.get_cursor(0, Side::Middle).is_none());
    list.push("a")?;
    list.push("b")?;
    assert_eq!(list.pop()?, Some("b".into()));
    assert_eq!(list.get_deep_value().to_json_value(), json!(["a"]));

    let movable = LoroMovableList::new();
    assert!(movable.pop()?.is_none());
    movable.delete(0, 0)?;
    assert!(movable.get_cursor(0, Side::Middle).is_none());
    movable.push("a")?;
    movable.push("b")?;
    movable.mov(0, 1)?;
    assert_eq!(movable.get_deep_value().to_json_value(), json!(["b", "a"]));
    assert_eq!(
        movable
            .pop()?
            .expect("last item should exist")
            .get_deep_value(),
        "a".into()
    );
    assert_eq!(movable.get_deep_value().to_json_value(), json!(["b"]));

    Ok(())
}

#[test]
fn detached_list_and_movable_list_container_boundaries_return_errors() -> LoroResult<()> {
    let list = LoroList::new();
    assert!(list.insert_container(1, LoroText::new()).is_err());
    let text = list.insert_container(0, LoroText::new())?;
    text.insert(0, "nested")?;
    assert!(list.insert_container(2, LoroMap::new()).is_err());
    assert_eq!(list.get_deep_value().to_json_value(), json!(["nested"]));

    let movable = LoroMovableList::new();
    assert!(movable.insert_container(1, LoroMap::new()).is_err());
    let map = movable.insert_container(0, LoroMap::new())?;
    map.insert("kind", "original")?;
    assert!(movable.set_container(1, LoroText::new()).is_err());
    let text = movable.set_container(0, LoroText::new())?;
    text.insert(0, "replacement")?;
    assert_eq!(
        movable.get_deep_value().to_json_value(),
        json!(["replacement"])
    );

    Ok(())
}
