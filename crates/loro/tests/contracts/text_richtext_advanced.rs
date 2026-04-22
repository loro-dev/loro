use loro::{
    cursor::{PosType, Side},
    ExpandType, ExportMode, LoroDoc, LoroResult, StyleConfig, StyleConfigMap, TextDelta, ToJson,
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
fn richtext_mark_slice_unmark_and_encoding_conversions_follow_contract() -> LoroResult<()> {
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

#[test]
fn richtext_expand_modes_apply_at_documented_boundaries() -> LoroResult<()> {
    let doc = LoroDoc::new();
    let mut styles = StyleConfigMap::new();
    styles.insert("after".into(), StyleConfig::new().expand(ExpandType::After));
    styles.insert("none".into(), StyleConfig::new().expand(ExpandType::None));
    doc.config_text_style(styles);

    let after = doc.get_text("after");
    after.insert(0, "ab")?;
    after.mark(0..2, "after", true)?;
    doc.commit();
    after.insert(2, "R")?;
    assert_eq!(
        after.to_delta(),
        vec![TextDelta::Insert {
            insert: "abR".to_string(),
            attributes: Some([("after".to_string(), true.into())].into_iter().collect()),
        }]
    );

    let none = doc.get_text("none");
    none.insert(0, "ab")?;
    none.mark(0..2, "none", true)?;
    doc.commit();
    none.insert(0, "L")?;
    none.insert(none.len_unicode(), "R")?;
    assert_eq!(
        none.to_delta(),
        vec![
            TextDelta::Insert {
                insert: "L".to_string(),
                attributes: None,
            },
            TextDelta::Insert {
                insert: "ab".to_string(),
                attributes: Some([("none".to_string(), true.into())].into_iter().collect()),
            },
            TextDelta::Insert {
                insert: "R".to_string(),
                attributes: None,
            },
        ]
    );

    Ok(())
}

#[test]
fn richtext_before_and_both_expand_types_follow_documented_boundaries() -> LoroResult<()> {
    let before_doc = LoroDoc::new();
    let mut styles = StyleConfigMap::default_rich_text_config();
    styles.insert(
        "before".into(),
        StyleConfig::new().expand(ExpandType::Before),
    );
    styles.insert("both".into(), StyleConfig::new().expand(ExpandType::Both));
    before_doc.config_text_style(styles.clone());

    let before = before_doc.get_text("before");
    before.insert(0, "ab")?;
    before.mark(0..2, "before", true)?;
    before_doc.commit();
    before.insert(0, "L")?;
    before.insert(before.len_unicode(), "R")?;
    assert_eq!(
        before.to_delta(),
        vec![
            TextDelta::Insert {
                insert: "Lab".to_string(),
                attributes: Some([("before".to_string(), true.into())].into_iter().collect()),
            },
            TextDelta::Insert {
                insert: "R".to_string(),
                attributes: None,
            },
        ]
    );

    let both_doc = LoroDoc::new();
    both_doc.config_text_style(styles);
    let both = both_doc.get_text("both");
    both.insert(0, "ab")?;
    both.mark(0..2, "both", true)?;
    both_doc.commit();
    both.insert(0, "L")?;
    both.insert(both.len_unicode(), "R")?;
    assert_eq!(
        both.to_delta(),
        vec![TextDelta::Insert {
            insert: "LabR".to_string(),
            attributes: Some([("both".to_string(), true.into())].into_iter().collect()),
        }]
    );
    assert_eq!(
        both.get_richtext_value().to_json_value(),
        serde_json::json!([
            { "insert": "LabR", "attributes": { "both": true } }
        ])
    );

    let snapshot = both_doc.export(ExportMode::Snapshot)?;
    let restored = LoroDoc::from_snapshot(&snapshot)?;
    assert_eq!(restored.get_text("both").to_delta(), both.to_delta());

    Ok(())
}

#[test]
fn richtext_default_style_config_applies_to_unconfigured_marks() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.config_default_text_style(Some(StyleConfig::new().expand(ExpandType::Both)));

    let text = doc.get_text("text");
    text.insert(0, "ab")?;
    text.mark(0..2, "custom_mark", true)?;
    doc.commit();

    text.insert(0, "L")?;
    text.insert(text.len_unicode(), "R")?;
    assert_eq!(
        text.to_delta(),
        vec![TextDelta::Insert {
            insert: "LabR".to_string(),
            attributes: Some(
                [("custom_mark".to_string(), true.into())]
                    .into_iter()
                    .collect(),
            ),
        }]
    );

    let restored = LoroDoc::from_snapshot(&doc.export(ExportMode::Snapshot)?)?;
    assert_eq!(restored.get_text("text").to_delta(), text.to_delta());

    Ok(())
}

#[test]
fn richtext_partial_unmark_unicode_and_snapshot_roundtrip_preserve_spans() -> LoroResult<()> {
    let doc = LoroDoc::new();
    let mut styles = StyleConfigMap::new();
    styles.insert("note".into(), StyleConfig::new().expand(ExpandType::After));
    doc.config_text_style(styles);

    let text = doc.get_text("text");
    text.insert(0, "a😀b文c")?;
    text.mark(0..text.len_unicode(), "note", "whole")?;
    text.unmark(1..4, "note")?;
    assert_eq!(
        text.to_delta(),
        vec![
            TextDelta::Insert {
                insert: "a".to_string(),
                attributes: Some([("note".to_string(), "whole".into())].into_iter().collect()),
            },
            TextDelta::Insert {
                insert: "😀b文".to_string(),
                attributes: None,
            },
            TextDelta::Insert {
                insert: "c".to_string(),
                attributes: Some([("note".to_string(), "whole".into())].into_iter().collect()),
            },
        ]
    );

    let snapshot = doc.export(ExportMode::Snapshot)?;
    let restored = LoroDoc::from_snapshot(&snapshot)?;
    assert_eq!(
        restored.get_text("text").to_delta(),
        doc.get_text("text").to_delta()
    );
    assert_eq!(
        restored
            .get_text("text")
            .get_richtext_value()
            .to_json_value(),
        doc.get_text("text").get_richtext_value().to_json_value()
    );

    Ok(())
}

#[test]
fn richtext_delete_across_overlapping_style_boundaries_keeps_public_delta_clean() -> LoroResult<()>
{
    let doc = LoroDoc::new();
    doc.config_text_style(StyleConfigMap::default_rich_text_config());

    let text = doc.get_text("text");
    text.insert(0, "abcdefghi")?;
    text.mark(1..7, "bold", true)?;
    text.mark(3..9, "link", "https://example.com")?;

    text.delete(2, 4)?;
    let expected = vec![
        TextDelta::Insert {
            insert: "a".to_string(),
            attributes: None,
        },
        TextDelta::Insert {
            insert: "b".to_string(),
            attributes: Some([("bold".to_string(), true.into())].into_iter().collect()),
        },
        TextDelta::Insert {
            insert: "g".to_string(),
            attributes: Some(
                [
                    ("bold".to_string(), true.into()),
                    ("link".to_string(), "https://example.com".into()),
                ]
                .into_iter()
                .collect(),
            ),
        },
        TextDelta::Insert {
            insert: "hi".to_string(),
            attributes: Some(
                [("link".to_string(), "https://example.com".into())]
                    .into_iter()
                    .collect(),
            ),
        },
    ];

    assert_eq!(text.to_string(), "abghi");
    assert_eq!(text.to_delta(), expected);
    assert_eq!(
        text.get_richtext_value().to_json_value(),
        serde_json::json!([
            { "insert": "a" },
            { "insert": "b", "attributes": { "bold": true } },
            { "insert": "g", "attributes": { "bold": true, "link": "https://example.com" } },
            { "insert": "hi", "attributes": { "link": "https://example.com" } }
        ])
    );

    Ok(())
}

#[test]
fn richtext_overlapping_styles_keep_neighbor_intersections_after_insert_and_snapshot(
) -> LoroResult<()> {
    let doc = LoroDoc::new();
    let styles = StyleConfigMap::default_rich_text_config();
    doc.config_text_style(styles);

    let text = doc.get_text("text");
    text.insert(0, "abcd")?;
    text.mark(0..4, "bold", true)?;
    text.mark(1..3, "link", "https://example.com")?;
    text.unmark(2..3, "bold")?;
    assert_eq!(
        text.to_delta(),
        vec![
            TextDelta::Insert {
                insert: "a".to_string(),
                attributes: Some([("bold".to_string(), true.into())].into_iter().collect()),
            },
            TextDelta::Insert {
                insert: "b".to_string(),
                attributes: Some(
                    [
                        ("bold".to_string(), true.into()),
                        ("link".to_string(), "https://example.com".into()),
                    ]
                    .into_iter()
                    .collect(),
                ),
            },
            TextDelta::Insert {
                insert: "c".to_string(),
                attributes: Some(
                    [("link".to_string(), "https://example.com".into())]
                        .into_iter()
                        .collect()
                ),
            },
            TextDelta::Insert {
                insert: "d".to_string(),
                attributes: Some([("bold".to_string(), true.into())].into_iter().collect()),
            },
        ]
    );
    assert_eq!(
        text.get_richtext_value().to_json_value(),
        serde_json::json!([
            { "insert": "a", "attributes": { "bold": true } },
            { "insert": "b", "attributes": { "bold": true, "link": "https://example.com" } },
            { "insert": "c", "attributes": { "link": "https://example.com" } },
            { "insert": "d", "attributes": { "bold": true } }
        ])
    );

    let snapshot = doc.export(ExportMode::Snapshot)?;
    let restored = LoroDoc::from_snapshot(&snapshot)?;
    let restored_text = restored.get_text("text");
    assert_eq!(restored_text.to_string(), text.to_string());
    assert_eq!(restored_text.to_delta(), text.to_delta());
    assert_eq!(
        restored_text.get_richtext_value().to_json_value(),
        text.get_richtext_value().to_json_value()
    );

    Ok(())
}
