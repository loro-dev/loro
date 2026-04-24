use loro::{
    cursor::{Cursor, PosType, Side},
    Container, ContainerTrait, ExportMode, LoroDoc, LoroList, LoroMap, LoroResult, LoroText,
    StyleConfig, StyleConfigMap, TextDelta, ToJson, ValueOrContainer,
};
use pretty_assertions::assert_eq;
use serde_json::json;

fn byte_pos(s: &str, char_index: usize) -> usize {
    s.char_indices()
        .nth(char_index)
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| s.len())
}

#[test]
fn utf8_edits_around_multibyte_text_preserve_delta_and_json_contracts() -> LoroResult<()> {
    let doc = LoroDoc::new();

    let mut styles = StyleConfigMap::default_rich_text_config();
    styles.insert(
        "mark".into(),
        StyleConfig::new().expand(loro::ExpandType::None),
    );
    doc.config_text_style(styles);

    let text = doc.get_text("text");
    let original = "a😀文b";
    text.insert_utf8(0, original)?;
    text.mark_utf8(byte_pos(original, 2)..byte_pos(original, 3), "mark", true)?;

    assert_eq!(text.len_utf8(), original.len());
    assert_eq!(text.len_unicode(), 4);
    assert_eq!(text.len_utf16(), 5);

    text.delete_utf8(byte_pos(original, 1), "😀".len())?;
    text.insert_utf8(1, "Ω")?;

    assert_eq!(text.to_string(), "aΩ文b");

    let expected = vec![
        TextDelta::Insert {
            insert: "aΩ".to_string(),
            attributes: None,
        },
        TextDelta::Insert {
            insert: "文".to_string(),
            attributes: Some([("mark".to_string(), true.into())].into_iter().collect()),
        },
        TextDelta::Insert {
            insert: "b".to_string(),
            attributes: None,
        },
    ];

    assert_eq!(text.to_delta(), expected);
    assert_eq!(
        text.get_richtext_value().to_json_value(),
        json!([
            { "insert": "aΩ" },
            { "insert": "文", "attributes": { "mark": true } },
            { "insert": "b" }
        ])
    );
    assert_eq!(
        text.slice_delta(0, text.len_utf8(), PosType::Bytes)?,
        expected
    );

    Ok(())
}

#[test]
fn styled_text_markers_do_not_shift_text_coordinates() -> LoroResult<()> {
    let doc = LoroDoc::new();

    let mut styles = StyleConfigMap::default_rich_text_config();
    styles.insert(
        "link".into(),
        StyleConfig::new().expand(loro::ExpandType::None),
    );
    doc.config_text_style(styles);

    let text = doc.get_text("text");
    text.insert(0, "a😀")?;
    text.mark(0..2, "link", true)?;
    doc.commit();

    assert_eq!(text.len_unicode(), 2);
    assert_eq!(text.len_utf16(), 3);
    assert_eq!(
        text.convert_pos(3, PosType::Utf16, PosType::Unicode),
        Some(2)
    );
    assert_eq!(
        text.convert_pos("a😀".len(), PosType::Bytes, PosType::Unicode),
        Some(2)
    );
    assert_eq!(
        text.convert_pos(2, PosType::Unicode, PosType::Utf16),
        Some(3)
    );

    text.insert_utf16(3, "b")?;
    assert_eq!(text.to_string(), "a😀b");
    assert_eq!(
        text.to_delta(),
        vec![
            TextDelta::Insert {
                insert: "a😀".to_string(),
                attributes: Some([("link".to_string(), true.into())].into_iter().collect()),
            },
            TextDelta::Insert {
                insert: "b".to_string(),
                attributes: None,
            },
        ]
    );

    Ok(())
}

#[test]
fn detached_children_attach_through_get_or_create_and_keep_identity() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(41)?;

    let root = doc.get_map("root");
    let detached_text = LoroText::new();
    let detached_list = LoroList::new();
    let detached_map = LoroMap::new();

    detached_text.insert(0, "hello 😀")?;
    detached_list.push("alpha")?;
    detached_list.push("beta")?;
    detached_map.insert("kind", "draft")?;

    assert!(!detached_text.is_attached());
    assert!(!detached_list.is_attached());
    assert!(!detached_map.is_attached());
    assert!(detached_text.doc().is_none());
    assert!(detached_list.doc().is_none());
    assert!(detached_map.doc().is_none());

    let bundle = root.get_or_create_container("bundle", LoroMap::new())?;
    let body = bundle.get_or_create_container("body", detached_text.clone())?;
    let items = bundle.get_or_create_container("items", detached_list.clone())?;
    let meta = bundle.get_or_create_container("meta", detached_map.clone())?;

    body.insert_utf8(body.len_utf8(), " world")?;
    items.push("gamma")?;
    meta.insert("version", 1)?;
    doc.commit();

    let bundle_again = root.get_or_create_container("bundle", LoroMap::new())?;
    let body_again = bundle_again.get_or_create_container("body", LoroText::new())?;
    let items_again = bundle_again.get_or_create_container("items", LoroList::new())?;
    let meta_again = bundle_again.get_or_create_container("meta", LoroMap::new())?;

    assert!(bundle.is_attached());
    assert!(body.is_attached());
    assert!(items.is_attached());
    assert!(meta.is_attached());
    assert_eq!(bundle.id(), bundle_again.id());
    assert_eq!(body.id(), body_again.id());
    assert_eq!(items.id(), items_again.id());
    assert_eq!(meta.id(), meta_again.id());

    let attached_body = detached_text
        .get_attached()
        .expect("detached text should resolve to its attached clone");
    let attached_items = detached_list
        .get_attached()
        .expect("detached list should resolve to its attached clone");
    let attached_meta = detached_map
        .get_attached()
        .expect("detached map should resolve to its attached clone");

    assert!(attached_body.is_attached());
    assert!(attached_items.is_attached());
    assert!(attached_meta.is_attached());
    assert_eq!(attached_body.id(), body.id());
    assert_eq!(attached_items.id(), items.id());
    assert_eq!(attached_meta.id(), meta.id());

    assert_eq!(
        root.get_deep_value().to_json_value(),
        json!({
            "bundle": {
                "body": "hello 😀 world",
                "items": ["alpha", "beta", "gamma"],
                "meta": { "kind": "draft", "version": 1 }
            }
        })
    );
    assert!(matches!(
        bundle.get("body").expect("body should exist"),
        ValueOrContainer::Container(Container::Text(_))
    ));
    assert!(matches!(
        bundle.get("items").expect("items should exist"),
        ValueOrContainer::Container(Container::List(_))
    ));
    assert!(matches!(
        bundle.get("meta").expect("meta should exist"),
        ValueOrContainer::Container(Container::Map(_))
    ));

    let restored = LoroDoc::from_snapshot(&doc.export(ExportMode::Snapshot)?)?;
    assert_eq!(
        restored.get_deep_value().to_json_value(),
        doc.get_deep_value().to_json_value()
    );
    assert_eq!(
        restored
            .get_container(body.id())
            .expect("body should roundtrip")
            .id(),
        body.id()
    );
    assert_eq!(
        restored
            .get_container(items.id())
            .expect("items should roundtrip")
            .id(),
        items.id()
    );
    assert_eq!(
        restored
            .get_container(meta.id())
            .expect("meta should roundtrip")
            .id(),
        meta.id()
    );

    Ok(())
}

#[test]
fn detached_and_attached_text_coordinate_apis_follow_the_same_contract() -> LoroResult<()> {
    let detached = LoroText::new();
    assert!(detached.is_empty());
    assert!(detached.apply_delta(&[]).is_err());

    let content = "A😀BC文";
    detached.insert(0, content)?;
    detached.mark(1..3, "bold", true)?;
    assert_eq!(detached.len_unicode(), 5);
    assert_eq!(detached.len_utf16(), 6);
    assert_eq!(detached.len_utf8(), content.len());
    assert_eq!(detached.char_at(1)?, '😀');
    assert_eq!(detached.slice(1, 4)?, "😀BC");
    assert_eq!(detached.slice_utf16(1, 3)?, "😀");
    assert!(detached.slice(3, 2).is_err());
    assert!(detached.char_at(99).is_err());

    let middle_delta = detached.slice_delta(1, 4, PosType::Unicode)?;
    assert_eq!(
        middle_delta,
        vec![
            TextDelta::Insert {
                insert: "😀B".to_string(),
                attributes: Some([("bold".to_string(), true.into())].into_iter().collect()),
            },
            TextDelta::Insert {
                insert: "C".to_string(),
                attributes: None,
            },
        ]
    );

    assert_eq!(detached.splice(2, 2, "xy")?, "BC");
    detached.splice_utf16(1, 2, "🙂")?;
    assert_eq!(detached.to_string(), "A🙂xy文");
    detached.delete_utf8(byte_pos(&detached.to_string(), 4), "文".len())?;
    assert_eq!(detached.to_string(), "A🙂xy");

    let doc = LoroDoc::new();
    doc.set_peer_id(42)?;
    let attached = doc.get_text("text");
    attached.insert(0, content)?;
    attached.mark(1..3, "bold", true)?;
    assert_eq!(attached.len_unicode(), 5);
    assert_eq!(attached.len_utf16(), 6);
    assert_eq!(attached.len_utf8(), content.len());
    assert_eq!(attached.char_at(1)?, '😀');
    assert_eq!(attached.slice(1, 4)?, "😀BC");
    assert_eq!(attached.slice_utf16(1, 3)?, "😀");
    assert!(attached.slice(3, 2).is_err());
    assert!(attached.char_at(99).is_err());

    let mut visited = String::new();
    attached.iter(|chunk| {
        visited.push_str(chunk);
        visited.len() < 2
    });
    assert!(visited.starts_with('A'));

    attached.delete(0, attached.len_unicode())?;
    attached.insert(0, "ABC文")?;
    attached.apply_delta(&[
        TextDelta::Retain {
            retain: 1,
            attributes: None,
        },
        TextDelta::Delete { delete: 1 },
        TextDelta::Insert {
            insert: "xy".to_string(),
            attributes: Some([("bold".to_string(), true.into())].into_iter().collect()),
        },
    ])?;
    assert_eq!(attached.to_string(), "AxyC文");
    assert_eq!(
        attached.slice_delta(1, 3, PosType::Unicode)?,
        vec![TextDelta::Insert {
            insert: "xy".to_string(),
            attributes: Some([("bold".to_string(), true.into())].into_iter().collect()),
        }]
    );

    Ok(())
}

#[test]
fn text_coordinate_error_and_event_position_contracts_are_consistent() -> LoroResult<()> {
    let doc = LoroDoc::new();

    let mut styles = StyleConfigMap::default_rich_text_config();
    styles.insert(
        "mark".into(),
        StyleConfig::new().expand(loro::ExpandType::Both),
    );
    doc.config_text_style(styles);

    let text = doc.get_text("text");
    text.insert(0, "A😀BC文")?;
    text.mark_utf16(1..4, "mark", true)?;

    for unicode_pos in 0..=text.len_unicode() {
        let event_pos = text
            .convert_pos(unicode_pos, PosType::Unicode, PosType::Event)
            .expect("unicode positions should convert to event positions");
        assert_eq!(
            text.convert_pos(event_pos, PosType::Event, PosType::Unicode),
            Some(unicode_pos)
        );
    }
    assert!(matches!(
        text.insert_utf8(2, "!"),
        Err(loro::LoroError::UTF8InUnicodeCodePoint { pos: 2 })
    ));
    assert!(matches!(
        text.insert_utf16(2, "!"),
        Err(loro::LoroError::UTF16InUnicodeCodePoint { pos: 2 })
    ));
    assert!(text.delete_utf8(2, 1).is_err());
    assert!(text.delete_utf16(2, 1).is_err());
    assert!(text
        .slice_delta(text.len_utf8() + 1, text.len_utf8() + 2, PosType::Bytes)
        .is_err());
    assert!(text
        .slice_delta(text.len_utf16() + 1, text.len_utf16() + 2, PosType::Utf16)
        .is_err());

    assert_eq!(
        text.slice_delta(2, 2, PosType::Unicode)?,
        Vec::<TextDelta>::new()
    );
    text.unmark_utf16(1..4, "mark")?;
    assert_eq!(
        text.to_delta(),
        vec![TextDelta::Insert {
            insert: "A😀BC文".to_string(),
            attributes: None,
        }]
    );

    let detached = LoroText::new();
    detached.push_str("xy")?;
    assert_eq!(detached.to_string(), "xy");

    Ok(())
}

#[test]
fn text_iter_stops_after_first_callback_and_works_after_snapshot() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.config_text_style(StyleConfigMap::default_rich_text_config());

    let text = doc.get_text("text");
    text.insert(0, "alpha beta gamma")?;
    text.mark(0..5, "bold", true)?;
    text.mark(6..10, "link", "https://example.com")?;

    let mut seen = Vec::new();
    text.iter(|chunk| {
        seen.push(chunk.to_string());
        false
    });
    assert_eq!(seen.len(), 1);
    assert!(!seen[0].is_empty());
    assert!(text.to_string().starts_with(&seen[0]));

    let restored = LoroDoc::from_snapshot(&doc.export(ExportMode::Snapshot)?)?;
    let restored_text = restored.get_text("text");
    let mut restored_seen = Vec::new();
    restored_text.iter(|chunk| {
        restored_seen.push(chunk.to_string());
        false
    });
    assert_eq!(restored_seen.len(), 1);
    assert!(!restored_seen[0].is_empty());
    assert!(restored_text.to_string().starts_with(&restored_seen[0]));
    assert_eq!(restored_text.to_string(), text.to_string());

    Ok(())
}

#[test]
fn text_iter_callback_can_access_doc_state() -> LoroResult<()> {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "abc")?;

    let mut seen = 0;
    text.iter(|chunk| {
        assert_eq!(text.to_string(), "abc");
        assert_eq!(chunk, "abc");
        seen += 1;
        true
    });

    assert_eq!(seen, 1);
    Ok(())
}

#[test]
fn empty_container_cursors_roundtrip_and_resolve_to_zero() -> LoroResult<()> {
    let doc = LoroDoc::new();

    let text = doc.get_text("text");
    let list = doc.get_list("list");
    let movable = doc.get_movable_list("movable");

    let text_cursor = text.get_cursor(0, Side::Middle).unwrap();
    let list_cursor = list.get_cursor(0, Side::Middle).unwrap();
    let movable_cursor = movable.get_cursor(0, Side::Middle).unwrap();

    assert_eq!(text_cursor.side, Side::Left);
    assert_eq!(list_cursor.side, Side::Left);
    assert_eq!(movable_cursor.side, Side::Left);

    for cursor in [&text_cursor, &list_cursor, &movable_cursor] {
        let decoded = Cursor::decode(&cursor.encode()).unwrap();
        assert_eq!(decoded, *cursor);
        let pos = doc
            .get_cursor_pos(&decoded)
            .expect("empty cursor should resolve");
        assert_eq!(pos.current.pos, 0);
        assert!(pos.update.is_none());
    }

    let text_right = text.get_cursor(0, Side::Right).unwrap();
    let list_right = list.get_cursor(0, Side::Right).unwrap();
    let movable_right = movable.get_cursor(0, Side::Right).unwrap();
    assert_eq!(text_right.side, Side::Right);
    assert_eq!(list_right.side, Side::Right);
    assert_eq!(movable_right.side, Side::Right);
    assert_eq!(
        doc.get_cursor_pos(&text_right)
            .expect("text cursor should resolve")
            .current
            .pos,
        0
    );
    assert_eq!(
        doc.get_cursor_pos(&list_right)
            .expect("list cursor should resolve")
            .current
            .pos,
        0
    );
    assert_eq!(
        doc.get_cursor_pos(&movable_right)
            .expect("movable cursor should resolve")
            .current
            .pos,
        0
    );

    Ok(())
}
