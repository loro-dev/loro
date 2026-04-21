use loro_common::{LoroError, LoroValue};
use loro_internal::{
    cursor::PosType,
    handler::{TextDelta, TextHandler, UpdateOptions},
    HandlerTrait, LoroDoc, ToJson,
};
use pretty_assertions::assert_eq;
use rustc_hash::FxHashMap;
use serde_json::json;

fn attrs(
    pairs: impl IntoIterator<Item = (&'static str, LoroValue)>,
) -> FxHashMap<String, LoroValue> {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

#[test]
fn attached_text_versions_styles_and_coordinate_errors_follow_contract(
) -> loro_internal::LoroResult<()> {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(78)?;
    let text = doc.get_text("text");

    let initial_version = text.version_id().expect("attached text has a version id");
    text.insert_unicode(0, "A😀文")?;
    assert_ne!(text.version_id(), Some(initial_version));
    assert_eq!(text.to_string(), "A😀文");

    assert!(matches!(
        text.insert_utf16(2, "x"),
        Err(LoroError::UTF16InUnicodeCodePoint { pos: 2 })
    ));
    assert!(matches!(
        text.insert_utf8(2, "x"),
        Err(LoroError::UTF8InUnicodeCodePoint { pos: 2 })
    ));
    assert_eq!(text.to_string(), "A😀文");
    assert_eq!(
        text.convert_pos(2, PosType::Unicode, PosType::Utf16),
        Some(3)
    );
    assert_eq!(
        text.convert_pos(5, PosType::Bytes, PosType::Unicode),
        Some(2)
    );
    assert_eq!(text.convert_pos(1, PosType::Unicode, PosType::Entity), None);
    assert_eq!(
        text.convert_pos(100, PosType::Unicode, PosType::Bytes),
        None
    );

    let before_missing_unmark = text.version_id();
    text.unmark(0, 1, "bold", PosType::Unicode)?;
    assert_eq!(text.version_id(), before_missing_unmark);

    text.mark(1, 3, "bold", true.into(), PosType::Unicode)?;
    let after_mark = text.version_id();
    text.mark(1, 3, "bold", true.into(), PosType::Unicode)?;
    assert_eq!(text.version_id(), after_mark);
    assert_eq!(
        text.get_richtext_value().to_json_value(),
        json!([
            { "insert": "A" },
            { "insert": "😀文", "attributes": { "bold": true } }
        ])
    );
    assert_eq!(
        text.slice_delta(0, text.len_unicode(), PosType::Unicode)?,
        vec![
            TextDelta::Insert {
                insert: "A".to_string(),
                attributes: None,
            },
            TextDelta::Insert {
                insert: "😀文".to_string(),
                attributes: Some(attrs([("bold", true.into())])),
            },
        ]
    );

    text.unmark(2, 3, "bold", PosType::Unicode)?;
    assert_eq!(
        text.get_richtext_value().to_json_value(),
        json!([
            { "insert": "A" },
            { "insert": "😀", "attributes": { "bold": true } },
            { "insert": "文" }
        ])
    );

    text.check();
    text.diagnose();
    Ok(())
}

#[test]
fn apply_delta_and_update_keep_text_and_attribute_contracts() -> loro_internal::LoroResult<()> {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(79)?;
    let text = doc.get_text("patch");

    text.apply_delta(&[TextDelta::Insert {
        insert: "hello".to_string(),
        attributes: Some(attrs([("bold", true.into())])),
    }])?;
    assert_eq!(
        text.get_delta(),
        vec![TextDelta::Insert {
            insert: "hello".to_string(),
            attributes: Some(attrs([("bold", true.into())])),
        }]
    );

    text.apply_delta(&[
        TextDelta::Retain {
            retain: 2,
            attributes: Some(attrs([("italic", true.into())])),
        },
        TextDelta::Delete { delete: 2 },
        TextDelta::Insert {
            insert: "Y".to_string(),
            attributes: None,
        },
    ])?;
    assert_eq!(text.to_string(), "heYo");
    assert_eq!(
        text.get_richtext_value().to_json_value(),
        json!([
            { "insert": "he", "attributes": { "bold": true, "italic": true } },
            { "insert": "Y" },
            { "insert": "o", "attributes": { "bold": true } }
        ])
    );

    text.update("plain\nnew\ntext", UpdateOptions::default())
        .expect("character update should finish without timeout");
    assert_eq!(text.to_string(), "plain\nnew\ntext");
    text.update_by_line("plain\nline\ntext\n", UpdateOptions::default())
        .expect("line update should finish without timeout");
    assert_eq!(text.to_string(), "plain\nline\ntext\n");

    Ok(())
}

#[test]
fn detached_text_richtext_state_survives_attach() -> loro_internal::LoroResult<()> {
    let detached = TextHandler::new_detached();
    assert_eq!(detached.version_id(), None);
    assert!(detached.is_empty());
    assert!(detached.apply_delta(&[]).is_err());

    detached.insert_unicode(0, "abc文")?;
    detached.mark(1, 3, "bold", true.into(), PosType::Unicode)?;
    detached.mark(1, 3, "bold", true.into(), PosType::Unicode)?;
    detached.unmark(0, 1, "bold", PosType::Unicode)?;
    assert!(matches!(
        detached.char_at(99, PosType::Unicode),
        Err(LoroError::OutOfBound { .. })
    ));
    assert!(matches!(
        detached.slice(3, 2, PosType::Unicode),
        Err(LoroError::EndIndexLessThanStartIndex { .. })
    ));
    assert!(detached
        .mark(2, 2, "bold", true.into(), PosType::Unicode)
        .is_err());
    assert_eq!(detached.slice(0, 0, PosType::Unicode)?, "");

    let expected_delta = vec![
        TextDelta::Insert {
            insert: "a".to_string(),
            attributes: None,
        },
        TextDelta::Insert {
            insert: "bc".to_string(),
            attributes: Some(attrs([("bold", true.into())])),
        },
        TextDelta::Insert {
            insert: "文".to_string(),
            attributes: None,
        },
    ];
    assert_eq!(detached.get_delta(), expected_delta);

    let mut first_chunk = String::new();
    detached.iter(|chunk| {
        first_chunk.push_str(chunk);
        false
    });
    assert_eq!(first_chunk, "a");
    detached.check();
    detached.diagnose();

    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(80)?;
    let root = doc.get_map("root");
    let attached = root.insert_container("body", detached.clone())?;
    let attached_from_detached = detached
        .get_attached()
        .expect("detached text should resolve to attached handler after insertion");
    assert_eq!(attached.id(), attached_from_detached.id());
    assert!(attached.version_id().is_some());
    assert_eq!(attached.get_delta(), expected_delta);
    assert_eq!(
        root.get_deep_value().to_json_value(),
        json!({ "body": "abc文" })
    );

    Ok(())
}
