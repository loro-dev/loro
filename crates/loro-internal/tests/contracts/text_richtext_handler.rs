use loro_internal::{
    cursor::{PosType, Side},
    handler::Handler,
    handler::TextDelta,
    ContainerType, HandlerTrait, LoroDoc, LoroResult, TextHandler, ToJson,
};
use pretty_assertions::assert_eq;
use serde_json::json;

fn richtext_json(text: &TextHandler) -> serde_json::Value {
    text.get_richtext_value().to_json_value()
}

#[test]
fn detached_text_handler_contract_covers_local_branch() -> LoroResult<()> {
    let text = TextHandler::new_detached();

    assert!(!text.is_attached());
    assert!(text.doc().is_none());
    assert!(text.version_id().is_none());
    assert!(text.get_attached().is_none());
    assert!(text.is_empty());
    assert_eq!(format!("{:?}", text), "TextHandler(Unattached)");

    text.insert_utf8(0, "a😀文b")?;
    text.mark(1, 3, "bold", true.into(), PosType::Unicode)?;

    assert_eq!(text.len_utf8(), 9);
    assert_eq!(text.len_unicode(), 4);
    assert_eq!(text.len_utf16(), 5);
    let event_len = if cfg!(feature = "wasm") { 5 } else { 4 };
    assert_eq!(text.len_event(), event_len);
    assert_eq!(text.char_at(1, PosType::Unicode)?, '😀');
    assert_eq!(text.slice(1, 3, PosType::Unicode)?, "😀文");
    assert_eq!(text.slice_utf16(1, 4)?, "😀文");
    assert_eq!(
        text.convert_pos(2, PosType::Unicode, PosType::Bytes),
        Some(5)
    );
    assert_eq!(
        text.convert_pos(5, PosType::Bytes, PosType::Unicode),
        Some(2)
    );
    assert_eq!(
        text.convert_pos(1, PosType::Unicode, PosType::Utf16),
        Some(1)
    );
    assert!(text.char_at(99, PosType::Unicode).is_err());
    assert!(text.slice(3, 2, PosType::Unicode).is_err());

    let mut seen = Vec::new();
    text.iter(|chunk| {
        seen.push(chunk.to_owned());
        false
    });
    assert_eq!(seen, vec!["a".to_string()]);

    assert_eq!(
        richtext_json(&text),
        json!([
            {"insert": "a"},
            {"insert": "😀文", "attributes": {"bold": true}},
            {"insert": "b"}
        ])
    );
    assert_eq!(
        text.slice_delta(0, text.len_unicode(), PosType::Unicode)?,
        vec![
            TextDelta::Insert {
                insert: "a".to_string(),
                attributes: None,
            },
            TextDelta::Insert {
                insert: "😀文".to_string(),
                attributes: Some([("bold".to_string(), true.into())].into_iter().collect()),
            },
            TextDelta::Insert {
                insert: "b".to_string(),
                attributes: None,
            },
        ]
    );

    let handler = text.to_handler();
    assert_eq!(handler.c_type(), ContainerType::Text);
    assert!(!handler.is_attached());
    assert!(handler.attached_handler().is_none());
    assert!(handler.doc().is_none());
    assert_eq!(handler.get_value().to_json_value(), json!("a😀文b"));
    assert_eq!(handler.get_deep_value().to_json_value(), json!("a😀文b"));
    assert!(<TextHandler as HandlerTrait>::from_handler(handler.clone()).is_some());
    assert!(<Handler as HandlerTrait>::from_handler(handler).is_some());

    text.diagnose();

    let plain = TextHandler::new_detached();
    plain.insert_utf8(0, "clear me")?;
    plain.clear()?;
    assert_eq!(plain.len_unicode(), 0);
    assert_eq!(plain.len_utf8(), 0);
    assert_eq!(richtext_json(&plain), json!([]));

    Ok(())
}

#[test]
fn attached_text_handler_contract_covers_document_branch_and_handler_enum() -> LoroResult<()> {
    let doc = LoroDoc::new_auto_commit();
    let root = doc.get_map("root");

    let detached = TextHandler::new_detached();
    detached.insert_utf8(0, "abcd")?;
    detached.mark(1, 3, "bold", true.into(), PosType::Unicode)?;

    let attached = root.insert_container("body", detached.clone())?;

    assert!(!detached.is_attached());
    assert!(detached.doc().is_none());
    assert!(detached.get_attached().is_some());
    assert!(attached.is_attached());
    assert!(attached.doc().is_some());
    assert!(attached.version_id().is_some());
    assert_eq!(attached.len_utf8(), 4);
    assert_eq!(attached.len_unicode(), 4);
    assert_eq!(attached.len_utf16(), 4);
    assert_eq!(attached.char_at(1, PosType::Unicode)?, 'b');
    assert_eq!(attached.slice(1, 3, PosType::Unicode)?, "bc");
    assert_eq!(attached.slice_utf16(1, 3)?, "bc");
    assert_eq!(
        attached.convert_pos(2, PosType::Unicode, PosType::Bytes),
        Some(2)
    );
    assert_eq!(
        attached.convert_pos(2, PosType::Bytes, PosType::Unicode),
        Some(2)
    );

    let mut seen = Vec::new();
    attached.iter(|chunk| {
        seen.push(chunk.to_owned());
        false
    });
    assert_eq!(seen, vec!["a".to_string()]);

    assert_eq!(
        attached.slice_delta(0, attached.len_unicode(), PosType::Unicode)?,
        vec![
            TextDelta::Insert {
                insert: "a".to_string(),
                attributes: None,
            },
            TextDelta::Insert {
                insert: "bc".to_string(),
                attributes: Some([("bold".to_string(), true.into())].into_iter().collect()),
            },
            TextDelta::Insert {
                insert: "d".to_string(),
                attributes: None,
            },
        ]
    );
    assert_eq!(
        attached.get_richtext_value().to_json_value(),
        json!([
            {"insert": "a"},
            {"insert": "bc", "attributes": {"bold": true}},
            {"insert": "d"}
        ])
    );
    assert_eq!(
        root.get_deep_value().to_json_value(),
        json!({"body": "abcd"})
    );

    let handler = attached.to_handler();
    assert_eq!(handler.kind(), ContainerType::Text);
    assert!(handler.is_attached());
    assert!(handler.attached_handler().is_some());
    assert!(handler.doc().is_some());
    assert_eq!(handler.get_value().to_json_value(), json!("abcd"));
    assert_eq!(handler.get_deep_value().to_json_value(), json!("abcd"));
    assert!(<TextHandler as HandlerTrait>::from_handler(handler.clone()).is_some());
    assert!(<Handler as HandlerTrait>::from_handler(handler.clone()).is_some());

    let before = attached.version_id();
    attached.insert_utf8(attached.len_utf8(), "!")?;
    attached.delete_utf16(1, 2)?;
    assert_ne!(attached.version_id(), before);
    assert_eq!(
        attached.slice(0, attached.len_unicode(), PosType::Unicode)?,
        "ad!"
    );
    assert_eq!(
        root.get_deep_value().to_json_value(),
        json!({"body": "ad!"})
    );

    handler.clear()?;
    assert_eq!(attached.len_unicode(), 0);
    assert_eq!(attached.len_utf8(), 0);
    assert!(attached.version_id().is_some());
    let cursor = attached
        .get_cursor(0, Side::Middle)
        .expect("empty attached text");
    assert_eq!(cursor.id, None);
    assert_eq!(cursor.side, Side::Left);
    assert_eq!(attached.char_at(0, PosType::Unicode).is_err(), true);
    assert_eq!(attached.get_richtext_value().to_json_value(), json!([]));

    attached.diagnose();
    assert!(attached.char_at(0, PosType::Unicode).is_err());
    assert!(attached.slice(1, 0, PosType::Unicode).is_err());

    Ok(())
}

#[test]
fn text_handler_position_edge_cases_cover_unsupported_and_empty_ranges() -> LoroResult<()> {
    let text = TextHandler::new_detached();
    text.insert_utf8(0, "ab")?;

    assert_eq!(
        text.get_delta(),
        vec![TextDelta::Insert {
            insert: "ab".to_string(),
            attributes: None,
        }]
    );
    assert_eq!(
        text.convert_pos(0, PosType::Unicode, PosType::Unicode),
        Some(0)
    );
    assert_eq!(text.convert_pos(0, PosType::Entity, PosType::Unicode), None);
    assert_eq!(text.convert_pos(0, PosType::Unicode, PosType::Entity), None);
    assert_eq!(text.convert_pos(10, PosType::Unicode, PosType::Bytes), None);
    assert_eq!(text.slice(1, 1, PosType::Unicode)?, "");
    assert_eq!(text.slice_utf16(1, 1)?, "");

    let doc = LoroDoc::new_auto_commit();
    let root = doc.get_map("root");
    let attached = root.insert_container("body", text.clone())?;

    assert!(attached.is_attached());
    assert_eq!(
        attached.get_delta(),
        vec![TextDelta::Insert {
            insert: "ab".to_string(),
            attributes: None,
        }]
    );
    assert_eq!(
        attached.convert_pos(0, PosType::Entity, PosType::Unicode),
        None
    );
    assert_eq!(
        attached.convert_pos(attached.len_unicode(), PosType::Unicode, PosType::Unicode),
        Some(attached.len_unicode())
    );
    assert_eq!(
        attached.convert_pos(attached.len_unicode() + 1, PosType::Unicode, PosType::Bytes),
        None
    );
    assert_eq!(attached.slice(0, 0, PosType::Unicode)?, "");

    Ok(())
}
