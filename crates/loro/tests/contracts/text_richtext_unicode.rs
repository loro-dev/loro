use loro::{
    cursor::PosType, ExportMode, LoroDoc, LoroResult, StyleConfig, StyleConfigMap, TextDelta,
};
use pretty_assertions::assert_eq;

fn configure_styles(doc: &LoroDoc, keys: &[&str]) {
    let mut styles = StyleConfigMap::default_rich_text_config();
    for key in keys {
        styles.insert((*key).into(), StyleConfig::new());
    }
    doc.config_text_style(styles);
}

fn attr_delta(text: &str, key: &str) -> TextDelta {
    TextDelta::Insert {
        insert: text.to_string(),
        attributes: Some([(key.to_string(), true.into())].into_iter().collect()),
    }
}

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
fn richtext_mixed_unicode_slices_and_deltas_follow_position_contracts() -> LoroResult<()> {
    let doc = LoroDoc::new();
    configure_styles(&doc, &["accent"]);
    let text = doc.get_text("text");
    let content = "a😀文𐐷b";
    text.insert(0, content)?;
    text.mark(1..4, "accent", true)?;

    assert_eq!(text.len_unicode(), 5);
    assert_eq!(text.len_utf8(), content.len());
    assert_eq!(text.len_utf16(), 7);
    assert_eq!(text.slice(1, 4)?, "😀文𐐷");
    assert_eq!(text.slice_utf16(1, 6)?, "😀文𐐷");
    assert_eq!(text.char_at(0)?, 'a');
    assert_eq!(text.char_at(3)?, '𐐷');

    assert_eq!(
        text.convert_pos(3, PosType::Unicode, PosType::Bytes),
        Some(8)
    );
    assert_eq!(
        text.convert_pos(4, PosType::Unicode, PosType::Utf16),
        Some(6)
    );
    assert_eq!(
        text.convert_pos(6, PosType::Utf16, PosType::Unicode),
        Some(4)
    );
    assert_eq!(
        text.convert_pos(13, PosType::Bytes, PosType::Unicode),
        Some(5)
    );
    assert_eq!(text.convert_pos(14, PosType::Bytes, PosType::Unicode), None);

    assert_eq!(
        text.slice_delta(1, 12, PosType::Bytes)?,
        vec![attr_delta("😀文𐐷", "accent")]
    );
    assert_eq!(
        text.slice_delta(1, 6, PosType::Utf16)?,
        vec![attr_delta("😀文𐐷", "accent")]
    );
    assert_eq!(
        text.slice_delta(2, 4, PosType::Unicode)?,
        vec![attr_delta("文𐐷", "accent")]
    );

    Ok(())
}

#[test]
fn slice_delta_on_multibyte_prefix_suffix_and_middle_preserves_attributes() -> LoroResult<()> {
    let doc = LoroDoc::new();
    configure_styles(&doc, &["warm", "wide"]);
    let text = doc.get_text("text");
    let content = "α😀文β";
    text.insert(0, content)?;
    text.mark_utf8(byte_pos(content, 0)..byte_pos(content, 2), "warm", true)?;
    text.mark_utf16(utf16_pos(content, 1)..utf16_pos(content, 3), "wide", "yes")?;

    assert_eq!(
        text.slice_delta(0, utf16_pos(content, 2), PosType::Utf16)?,
        vec![
            attr_delta("α", "warm"),
            TextDelta::Insert {
                insert: "😀".to_string(),
                attributes: Some(
                    [
                        ("warm".to_string(), true.into()),
                        ("wide".to_string(), "yes".into()),
                    ]
                    .into_iter()
                    .collect(),
                ),
            },
        ]
    );
    assert_eq!(
        text.slice_delta(byte_pos(content, 1), content.len(), PosType::Bytes)?,
        vec![
            TextDelta::Insert {
                insert: "😀".to_string(),
                attributes: Some(
                    [
                        ("warm".to_string(), true.into()),
                        ("wide".to_string(), "yes".into()),
                    ]
                    .into_iter()
                    .collect(),
                ),
            },
            TextDelta::Insert {
                insert: "文".to_string(),
                attributes: Some([("wide".to_string(), "yes".into())].into_iter().collect()),
            },
            TextDelta::Insert {
                insert: "β".to_string(),
                attributes: None,
            },
        ]
    );
    assert_eq!(
        text.slice_delta(1, 3, PosType::Unicode)?,
        vec![
            TextDelta::Insert {
                insert: "😀".to_string(),
                attributes: Some(
                    [
                        ("warm".to_string(), true.into()),
                        ("wide".to_string(), "yes".into()),
                    ]
                    .into_iter()
                    .collect(),
                ),
            },
            TextDelta::Insert {
                insert: "文".to_string(),
                attributes: Some([("wide".to_string(), "yes".into())].into_iter().collect()),
            },
        ]
    );

    Ok(())
}

#[test]
fn richtext_snapshot_queries_then_mutations_keep_unicode_contracts() -> LoroResult<()> {
    let doc = LoroDoc::new();
    configure_styles(&doc, &["mark", "prefix"]);
    let text = doc.get_text("text");
    text.insert(0, "Hi 😀文!")?;
    text.mark(3..5, "mark", true)?;
    doc.commit();

    let restored = LoroDoc::from_snapshot(&doc.export(ExportMode::Snapshot)?)?;
    configure_styles(&restored, &["mark", "prefix"]);
    let restored_text = restored.get_text("text");
    assert_eq!(restored_text.to_string(), "Hi 😀文!");
    assert_eq!(restored_text.len_unicode(), 6);
    assert_eq!(restored_text.len_utf16(), 7);
    assert_eq!(
        restored_text.slice_delta(3, 5, PosType::Unicode)?,
        vec![attr_delta("😀文", "mark")]
    );

    restored_text.insert(5, "X")?;
    restored_text.delete(0, 1)?;
    restored_text.mark_utf16(0..2, "prefix", true)?;
    assert_eq!(restored_text.to_string(), "i 😀文X!");
    assert_eq!(
        restored_text.slice_delta(0, restored_text.len_unicode(), PosType::Unicode)?,
        vec![
            attr_delta("i ", "prefix"),
            attr_delta("😀文", "mark"),
            TextDelta::Insert {
                insert: "X!".to_string(),
                attributes: None,
            },
        ]
    );

    let replica = LoroDoc::new();
    configure_styles(&replica, &["mark", "prefix"]);
    replica.import(&restored.export(ExportMode::all_updates())?)?;
    assert_eq!(replica.get_text("text").to_string(), "i 😀文X!");

    Ok(())
}
