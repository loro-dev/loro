use loro::{
    cursor::{Cursor, Side},
    LoroDoc, LoroMap, ToJson,
};
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn cursors_resolve_after_target_content_is_deleted_for_text_list_and_movable_list(
) -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(121)?;

    let text = doc.get_text("text");
    text.insert(0, "abcdef")?;
    let text_cursor = text.get_cursor(3, Side::Middle).unwrap();
    let text_end = text.get_cursor(text.len_unicode(), Side::Right).unwrap();

    let list = doc.get_list("list");
    list.push("a")?;
    list.push("b")?;
    list.push("c")?;
    list.push("d")?;
    let list_cursor = list.get_cursor(2, Side::Middle).unwrap();
    let list_end = list.get_cursor(list.len(), Side::Right).unwrap();

    let movable = doc.get_movable_list("movable");
    movable.push("a")?;
    movable.push("b")?;
    movable.push("c")?;
    movable.push("d")?;
    let movable_cursor = movable.get_cursor(2, Side::Middle).unwrap();
    let movable_end = movable.get_cursor(movable.len(), Side::Right).unwrap();
    doc.commit();

    assert_eq!(doc.get_cursor_pos(&text_cursor).unwrap().current.pos, 3);
    assert_eq!(doc.get_cursor_pos(&text_end).unwrap().current.pos, 6);
    assert_eq!(doc.get_cursor_pos(&list_cursor).unwrap().current.pos, 2);
    assert_eq!(doc.get_cursor_pos(&list_end).unwrap().current.pos, 4);
    assert_eq!(doc.get_cursor_pos(&movable_cursor).unwrap().current.pos, 2);
    assert_eq!(doc.get_cursor_pos(&movable_end).unwrap().current.pos, 4);

    text.delete(1, 4)?;
    list.delete(1, 2)?;
    movable.delete(1, 2)?;
    doc.commit();

    assert_eq!(text.to_string(), "af");
    assert_eq!(list.get_deep_value().to_json_value(), json!(["a", "d"]));
    assert_eq!(movable.get_deep_value().to_json_value(), json!(["a", "d"]));

    let text_pos = doc.get_cursor_pos(&text_cursor).unwrap();
    assert_eq!(text_pos.current.pos, 1);
    assert!(text_pos.update.is_some());
    assert_eq!(doc.get_cursor_pos(&text_end).unwrap().current.pos, 2);

    let list_pos = doc.get_cursor_pos(&list_cursor).unwrap();
    assert_eq!(list_pos.current.pos, 1);
    assert!(list_pos.update.is_some());
    assert_eq!(doc.get_cursor_pos(&list_end).unwrap().current.pos, 2);

    let movable_pos = doc.get_cursor_pos(&movable_cursor).unwrap();
    assert_eq!(movable_pos.current.pos, 1);
    assert!(movable_pos.update.is_some());
    assert_eq!(doc.get_cursor_pos(&movable_end).unwrap().current.pos, 2);

    text.insert(1, "XYZ")?;
    list.insert(1, "middle")?;
    movable.insert(1, "middle")?;
    doc.commit();

    assert_eq!(doc.get_cursor_pos(&text_cursor).unwrap().current.pos, 4);
    assert_eq!(doc.get_cursor_pos(&list_cursor).unwrap().current.pos, 2);
    assert_eq!(doc.get_cursor_pos(&movable_cursor).unwrap().current.pos, 2);
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "text": "aXYZf",
            "list": ["a", "middle", "d"],
            "movable": ["a", "middle", "d"],
        })
    );

    Ok(())
}

#[test]
fn cursor_encoding_side_values_and_cache_rebuilds_follow_contract() -> anyhow::Result<()> {
    assert_eq!(Side::from_i32(-1), Some(Side::Left));
    assert_eq!(Side::from_i32(0), Some(Side::Middle));
    assert_eq!(Side::from_i32(1), Some(Side::Right));
    assert_eq!(Side::from_i32(2), None);
    assert_eq!(Side::Left.to_i32(), -1);
    assert_eq!(Side::Middle.to_i32(), 0);
    assert_eq!(Side::Right.to_i32(), 1);

    let doc = LoroDoc::new();
    doc.set_peer_id(122)?;
    let text = doc.get_text("text");
    text.insert(0, "abcd")?;
    let list = doc.get_list("list");
    list.push("a")?;
    list.push("b")?;
    list.push("c")?;
    let movable = doc.get_movable_list("movable");
    movable.push("a")?;
    movable.push("b")?;
    movable.push("c")?;
    doc.commit();

    let text_cursor = text.get_cursor(2, Side::Middle).unwrap();
    let list_cursor = list.get_cursor(1, Side::Left).unwrap();
    let movable_cursor = movable.get_cursor(2, Side::Right).unwrap();

    for cursor in [&text_cursor, &list_cursor, &movable_cursor] {
        let decoded = Cursor::decode(&cursor.encode())?;
        assert_eq!(doc.get_cursor_pos(&decoded)?, doc.get_cursor_pos(cursor)?);
    }
    assert!(Cursor::decode(&[0xff, 0xff]).is_err());

    let checkpoint = doc.state_frontiers();
    text.insert(0, "zz")?;
    list.insert(0, "front")?;
    movable.insert(0, "front")?;
    doc.commit();

    assert_eq!(doc.get_cursor_pos(&text_cursor)?.current.pos, 4);
    assert_eq!(doc.get_cursor_pos(&list_cursor)?.current.pos, 2);
    assert_eq!(doc.get_cursor_pos(&movable_cursor)?.current.pos, 3);

    doc.free_diff_calculator();
    doc.checkout(&checkpoint)?;
    assert_eq!(doc.get_cursor_pos(&text_cursor)?.current.pos, 2);
    assert_eq!(doc.get_cursor_pos(&list_cursor)?.current.pos, 1);
    assert_eq!(doc.get_cursor_pos(&movable_cursor)?.current.pos, 2);

    Ok(())
}

#[test]
fn cursors_survive_parent_container_delete_while_history_is_available() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let root = doc.get_map("root");
    let holder = root.insert_container("holder", LoroMap::new())?;
    let text = holder.insert_container("text", loro::LoroText::new())?;
    text.insert(0, "abc")?;
    let cursor = text.get_cursor(1, Side::Middle).unwrap();
    doc.commit();

    assert_eq!(doc.get_cursor_pos(&cursor)?.current.pos, 1);
    root.delete("holder")?;
    doc.commit();

    assert_eq!(doc.get_cursor_pos(&cursor)?.current.pos, 1);

    Ok(())
}
