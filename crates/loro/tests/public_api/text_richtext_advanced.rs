use loro::{
    cursor::{PosType, Side},
    ExpandType, ExportMode, LoroDoc, LoroResult, StyleConfig, StyleConfigMap, TextDelta,
};
use pretty_assertions::assert_eq;

fn byte_pos(s: &str, char_index: usize) -> usize {
    s.char_indices()
        .nth(char_index)
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| s.len())
}

fn utf16_pos(s: &str, char_index: usize) -> usize {
    s.chars().take(char_index).map(|c| c.len_utf16()).sum()
}

#[test]
fn apply_delta_mixes_retain_insert_delete_and_ignores_empty_inserts() -> LoroResult<()> {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "ABCDE")?;

    text.apply_delta(&[
        TextDelta::Retain {
            retain: 1,
            attributes: Some([("bold".to_string(), true.into())].into_iter().collect()),
        },
        TextDelta::Insert {
            insert: String::new(),
            attributes: Some([("ghost".to_string(), true.into())].into_iter().collect()),
        },
        TextDelta::Insert {
            insert: "😀".to_string(),
            attributes: Some([("italic".to_string(), true.into())].into_iter().collect()),
        },
        TextDelta::Retain {
            retain: 2,
            attributes: Some([("code".to_string(), true.into())].into_iter().collect()),
        },
        TextDelta::Delete { delete: 1 },
        TextDelta::Retain {
            retain: 1,
            attributes: Some(
                [("underline".to_string(), true.into())]
                    .into_iter()
                    .collect(),
            ),
        },
    ])?;

    assert_eq!(text.to_string(), "A😀BCE");
    assert_eq!(
        text.to_delta(),
        vec![
            TextDelta::Insert {
                insert: "A".to_string(),
                attributes: Some([("bold".to_string(), true.into())].into_iter().collect()),
            },
            TextDelta::Insert {
                insert: "😀".to_string(),
                attributes: Some([("italic".to_string(), true.into())].into_iter().collect()),
            },
            TextDelta::Insert {
                insert: "BC".to_string(),
                attributes: Some([("code".to_string(), true.into())].into_iter().collect()),
            },
            TextDelta::Insert {
                insert: "E".to_string(),
                attributes: Some(
                    [("underline".to_string(), true.into())]
                        .into_iter()
                        .collect(),
                ),
            },
        ]
    );

    Ok(())
}

#[test]
fn richtext_mark_slice_unmark_and_encoding_conversions_follow_public_contract() -> LoroResult<()> {
    let doc = LoroDoc::new();
    let mut styles = StyleConfigMap::default_rich_text_config();
    styles.insert("after".into(), StyleConfig::new().expand(ExpandType::After));
    styles.insert(
        "underline".into(),
        StyleConfig::new().expand(ExpandType::After),
    );
    styles.insert("none".into(), StyleConfig::new().expand(ExpandType::None));
    doc.config_text_style(styles);

    let text = doc.get_text("text");
    let content = "a😀文b";
    text.insert(0, content)?;

    text.mark(1..2, "after", true)?;
    text.mark_utf8(byte_pos(content, 2)..byte_pos(content, 3), "none", true)?;
    text.mark_utf16(0..utf16_pos(content, 2), "underline", true)?;

    assert_eq!(text.slice(1, 3)?, "😀文");
    assert_eq!(text.slice_utf16(1, 4)?, "😀文");
    let byte_delta = text.slice_delta(1, byte_pos(content, 3), PosType::Bytes)?;
    assert_eq!(byte_delta.len(), 2);
    if let TextDelta::Insert { insert, attributes } = &byte_delta[0] {
        assert_eq!(insert, "😀");
        let attrs = attributes.as_ref().expect("emoji should keep attributes");
        assert!(attrs.contains_key("after"));
        assert!(attrs.contains_key("underline"));
    } else {
        unreachable!();
    }
    if let TextDelta::Insert { insert, attributes } = &byte_delta[1] {
        assert_eq!(insert, "文");
        let attrs = attributes.as_ref().expect("文 should keep attributes");
        assert!(attrs.contains_key("none"));
        assert!(!attrs.contains_key("after"));
        assert!(!attrs.contains_key("underline"));
    } else {
        unreachable!();
    }

    let unicode_delta = text.slice_delta(0, text.len_unicode(), PosType::Unicode)?;
    assert_eq!(unicode_delta.len(), 4);
    if let TextDelta::Insert { insert, attributes } = &unicode_delta[0] {
        assert_eq!(insert, "a");
        let attrs = attributes.as_ref().expect("a should carry underline");
        assert!(attrs.contains_key("underline"));
    } else {
        unreachable!();
    }
    if let TextDelta::Insert { insert, attributes } = &unicode_delta[1] {
        assert_eq!(insert, "😀");
        let attrs = attributes.as_ref().expect("emoji should carry attributes");
        assert!(attrs.contains_key("after"));
        assert!(attrs.contains_key("underline"));
    } else {
        unreachable!();
    }
    if let TextDelta::Insert { insert, attributes } = &unicode_delta[2] {
        assert_eq!(insert, "文");
        let attrs = attributes.as_ref().expect("文 should carry none");
        assert!(attrs.contains_key("none"));
        assert!(!attrs.contains_key("after"));
    } else {
        unreachable!();
    }
    if let TextDelta::Insert { insert, attributes } = &unicode_delta[3] {
        assert_eq!(insert, "b");
        assert!(attributes.is_none());
    } else {
        unreachable!();
    }
    assert_eq!(
        text.convert_pos(1, PosType::Unicode, PosType::Utf16),
        Some(1)
    );
    assert_eq!(
        text.convert_pos(2, PosType::Unicode, PosType::Utf16),
        Some(3)
    );
    assert_eq!(
        text.convert_pos(3, PosType::Utf16, PosType::Unicode),
        Some(2)
    );
    assert_eq!(
        text.convert_pos(5, PosType::Bytes, PosType::Unicode),
        Some(2)
    );
    assert_eq!(text.convert_pos(9, PosType::Bytes, PosType::Utf16), Some(5));
    assert_eq!(text.convert_pos(99, PosType::Unicode, PosType::Bytes), None);

    let after_doc = LoroDoc::new();
    let mut after_styles = StyleConfigMap::default_rich_text_config();
    after_styles.insert("after".into(), StyleConfig::new().expand(ExpandType::After));
    after_doc.config_text_style(after_styles);
    let after_text = after_doc.get_text("text");
    after_text.insert(0, content)?;
    after_text.mark(1..2, "after", true)?;
    after_text.insert(2, "X")?;
    assert_eq!(
        after_text.slice_delta(0, after_text.len_unicode(), PosType::Unicode)?,
        vec![
            TextDelta::Insert {
                insert: "a".to_string(),
                attributes: None,
            },
            TextDelta::Insert {
                insert: "😀X".to_string(),
                attributes: Some([("after".to_string(), true.into())].into_iter().collect()),
            },
            TextDelta::Insert {
                insert: "文b".to_string(),
                attributes: None,
            },
        ]
    );

    let none_doc = LoroDoc::new();
    let mut none_styles = StyleConfigMap::default_rich_text_config();
    none_styles.insert("none".into(), StyleConfig::new().expand(ExpandType::None));
    none_doc.config_text_style(none_styles);
    let none_text = none_doc.get_text("text");
    none_text.insert(0, content)?;
    none_text.mark_utf8(byte_pos(content, 2)..byte_pos(content, 3), "none", true)?;
    none_text.insert(3, "Y")?;
    assert_eq!(
        none_text.slice_delta(0, none_text.len_unicode(), PosType::Unicode)?,
        vec![
            TextDelta::Insert {
                insert: "a😀".to_string(),
                attributes: None,
            },
            TextDelta::Insert {
                insert: "文".to_string(),
                attributes: Some([("none".to_string(), true.into())].into_iter().collect()),
            },
            TextDelta::Insert {
                insert: "Yb".to_string(),
                attributes: None,
            },
        ]
    );

    let unmark_doc = LoroDoc::new();
    let unmark_text = unmark_doc.get_text("text");
    let unmark_content = "Hello world!";
    unmark_text.insert(0, unmark_content)?;
    unmark_text.mark(0..5, "bold", true)?;
    unmark_text.unmark(3..5, "bold")?;
    assert_eq!(
        unmark_text.to_delta(),
        vec![
            TextDelta::Insert {
                insert: "Hel".to_string(),
                attributes: Some([("bold".to_string(), true.into())].into_iter().collect()),
            },
            TextDelta::Insert {
                insert: "lo world!".to_string(),
                attributes: None,
            },
        ]
    );

    Ok(())
}

#[test]
fn cursor_editor_and_checkout_survive_remote_imports() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let text = doc.get_text("text");
    text.insert(0, "abcd")?;
    doc.commit();

    let checkpoint = doc.state_frontiers();
    let cursor = text.get_cursor(2, Side::Middle).unwrap();
    assert_eq!(
        doc.get_cursor_pos(&cursor)
            .expect("cursor should resolve")
            .current
            .pos,
        2
    );
    assert_eq!(text.get_editor_at_unicode_pos(0), Some(1));
    assert_eq!(text.get_editor_at_unicode_pos(1), Some(1));
    assert_eq!(text.get_editor_at_unicode_pos(2), Some(1));

    let remote = doc.fork();
    remote.set_peer_id(2)?;
    let remote_text = remote.get_text("text");
    remote_text.insert(0, "zz")?;
    remote.commit();

    let remote_updates = remote.export(ExportMode::updates(&doc.oplog_vv()))?;
    doc.import(&remote_updates)?;
    assert_eq!(text.to_string(), "zzabcd");
    assert_eq!(
        doc.get_cursor_pos(&cursor)
            .expect("cursor should resolve")
            .current
            .pos,
        4
    );
    assert_eq!(text.get_editor_at_unicode_pos(0), Some(2));
    assert_eq!(text.get_editor_at_unicode_pos(1), Some(2));
    assert_eq!(text.get_editor_at_unicode_pos(2), Some(1));

    doc.checkout(&checkpoint)?;
    assert_eq!(text.to_string(), "abcd");
    assert_eq!(
        doc.get_cursor_pos(&cursor)
            .expect("cursor should resolve")
            .current
            .pos,
        2
    );
    assert_eq!(text.get_editor_at_unicode_pos(0), Some(1));
    assert_eq!(text.get_editor_at_unicode_pos(1), Some(1));
    assert_eq!(text.get_editor_at_unicode_pos(2), Some(1));

    doc.checkout_to_latest();
    assert_eq!(
        doc.get_cursor_pos(&cursor)
            .expect("cursor should resolve")
            .current
            .pos,
        4
    );

    Ok(())
}
