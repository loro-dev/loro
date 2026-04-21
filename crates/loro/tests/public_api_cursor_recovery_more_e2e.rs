use loro::{cursor::Side, LoroDoc, ToJson};
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
