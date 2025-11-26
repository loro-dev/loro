use loro::cursor::PosType;
use loro::{ExpandType, LoroDoc, StyleConfig, StyleConfigMap, TextDelta};

// Convert a Unicode scalar index into a byte offset.
fn byte_pos(s: &str, char_index: usize) -> usize {
    s.char_indices()
        .nth(char_index)
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| s.len())
}

// Convert a Unicode scalar index into a UTF-16 code unit offset.
fn utf16_pos(s: &str, char_index: usize) -> usize {
    s.chars().take(char_index).map(|c| c.len_utf16()).sum()
}

#[test]
fn test_slice_delta() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "Hello world!").unwrap();
    text.mark(0..5, "bold", true).unwrap();
    
    // Test slice_delta
    let delta = text.slice_delta(0, 12, PosType::Unicode).unwrap();
    println!("{:?}", delta);
    
    assert_eq!(delta.len(), 2);
    
    match &delta[0] {
        TextDelta::Insert { insert, attributes } => {
             assert_eq!(insert, "Hello");
             let attrs = attributes.as_ref().unwrap();
             assert!(attrs.contains_key("bold"));
             assert_eq!(attrs.get("bold").unwrap(), &true.into());
        }
        _ => panic!("Expected Insert, got {:?}", delta[0]),
    }
    
    match &delta[1] {
        TextDelta::Insert { insert, attributes } => {
             assert_eq!(insert, " world!");
             assert!(attributes.is_none());
        }
        _ => panic!("Expected Insert, got {:?}", delta[1]),
    }
    
    // Test slice_delta with partial range
    let delta = text.slice_delta(2, 8, PosType::Unicode).unwrap();
    // "llo wo"
    // "llo" (bold), " wo" (no bold)
    assert_eq!(delta.len(), 2);
    match &delta[0] {
        TextDelta::Insert { insert, attributes } => {
             assert_eq!(insert, "llo");
             let attrs = attributes.as_ref().unwrap();
             assert!(attrs.contains_key("bold"));
        }
        _ => panic!("Expected Insert, got {:?}", delta[0]),
    }
    match &delta[1] {
        TextDelta::Insert { insert, attributes } => {
             assert_eq!(insert, " wo");
             assert!(attributes.is_none());
        }
        _ => panic!("Expected Insert, got {:?}", delta[1]),
    }
}

#[test]
fn test_slice_delta_overlapping() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "0123456789").unwrap();
    // "01234" bold
    text.mark(0..5, "bold", true).unwrap();
    // "23456" italic
    text.mark(2..7, "italic", true).unwrap();

    // Slice "1234567" (index 1 to 8)
    // 1: bold
    // 234: bold, italic
    // 56: italic
    // 7: none
    
    let delta = text.slice_delta(1, 8, PosType::Unicode).unwrap();
    assert_eq!(delta.len(), 4);

    // "1"
    if let TextDelta::Insert { insert, attributes } = &delta[0] {
        assert_eq!(insert, "1");
        let attrs = attributes.as_ref().unwrap();
        assert!(attrs.contains_key("bold"));
        assert!(!attrs.contains_key("italic"));
    } else { panic!("Expected segment 1") }

    // "234"
    if let TextDelta::Insert { insert, attributes } = &delta[1] {
        assert_eq!(insert, "234");
        let attrs = attributes.as_ref().unwrap();
        assert!(attrs.contains_key("bold"));
        assert!(attrs.contains_key("italic"));
    } else { panic!("Expected segment 234") }

    // "56"
    if let TextDelta::Insert { insert, attributes } = &delta[2] {
        assert_eq!(insert, "56");
        let attrs = attributes.as_ref().unwrap();
        assert!(!attrs.contains_key("bold"));
        assert!(attrs.contains_key("italic"));
    } else { panic!("Expected segment 56") }

    // "7"
    if let TextDelta::Insert { insert, attributes } = &delta[3] {
        assert_eq!(insert, "7");
        assert!(attributes.is_none());
    } else { panic!("Expected segment 7") }
}

#[test]
fn test_slice_delta_unicode() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    // "你好" len 2
    // "World" len 5
    text.insert(0, "你好World").unwrap();
    text.mark(0..2, "bold", true).unwrap(); // Mark "你好"

    // Slice "好W" (index 1 to 3)
    let delta = text.slice_delta(1, 3, PosType::Unicode).unwrap();
    assert_eq!(delta.len(), 2);
    
    // "好"
    if let TextDelta::Insert { insert, attributes } = &delta[0] {
        assert_eq!(insert, "好");
        assert!(attributes.as_ref().unwrap().contains_key("bold"));
    } else { panic!("Expected segment '好'") }

    // "W"
    if let TextDelta::Insert { insert, attributes } = &delta[1] {
        assert_eq!(insert, "W");
        assert!(attributes.is_none());
    } else { panic!("Expected segment 'W'") }
}

#[test]
fn test_slice_delta_with_deletion() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "01234").unwrap();
    text.mark(0..5, "bold", true).unwrap();
    text.delete(2, 2).unwrap(); // delete "23"
    // Now "014" (all bold)
    
    let delta = text.slice_delta(0, 3, PosType::Unicode).unwrap();
    assert_eq!(delta.len(), 1);
    
    if let TextDelta::Insert { insert, attributes } = &delta[0] {
        assert_eq!(insert, "014");
        assert!(attributes.as_ref().unwrap().contains_key("bold"));
    } else { panic!("Expected combined segment after deletion") }
}

#[test]
fn test_slice_delta_unicode_boundaries() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    // "😀" is 1 char (scalar) in Rust chars().
    text.insert(0, "A😀B").unwrap();
    
    // Mark "😀" (index 1 to 2)
    text.mark(1..2, "bold", true).unwrap();
    
    let delta = text.slice_delta(0, 3, PosType::Unicode).unwrap();
    assert_eq!(delta.len(), 3);
    
    // "A"
    if let TextDelta::Insert { insert, attributes } = &delta[0] {
        assert_eq!(insert, "A");
        assert!(attributes.is_none());
    } else { panic!("Expected 'A'") }
    
    // "😀"
    if let TextDelta::Insert { insert, attributes } = &delta[1] {
        assert_eq!(insert, "😀");
        assert!(attributes.as_ref().unwrap().contains_key("bold"));
    } else { panic!("Expected Emoji") }
    
    // "B"
    if let TextDelta::Insert { insert, attributes } = &delta[2] {
        assert_eq!(insert, "B");
        assert!(attributes.is_none());
    } else { panic!("Expected 'B'") }
}

#[test]
fn test_slice_delta_discontinuous_styles() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "AB").unwrap();
    text.mark(0..1, "bold", true).unwrap(); // A bold
    text.mark(1..2, "bold", true).unwrap(); // B bold
    // Even though they are applied separately, they might merge if they are adjacent and same.
    // Let's see if Loro merges adjacent identical styles.
    // Usually they should merge into one span if attributes are equal.
    
    let delta = text.slice_delta(0, 2, PosType::Unicode).unwrap();
    // Depends on implementation. Loro text delta usually merges adjacent same attributes.
    // If so, len is 1.
    if delta.len() == 1 {
         if let TextDelta::Insert { insert, attributes } = &delta[0] {
            assert_eq!(insert, "AB", "Expected merged segment 'AB', got '{}'", insert);
            assert!(attributes.as_ref().unwrap().contains_key("bold"));
        } else { panic!("Expected merged segment") }
    } else {
        // If not merged
        assert_eq!(delta.len(), 2, "Expected 1 or 2 segments, got {}", delta.len());
        if let TextDelta::Insert { insert, attributes } = &delta[0] {
            assert_eq!(insert, "A");
            assert!(attributes.as_ref().unwrap().contains_key("bold"));
        }
        if let TextDelta::Insert { insert, attributes } = &delta[1] {
            assert_eq!(insert, "B");
            assert!(attributes.as_ref().unwrap().contains_key("bold"));
        }
    }
}

#[test]
fn test_slice_delta_out_of_bounds() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "A").unwrap();
    // Slicing beyond end should error
    assert!(text.slice_delta(0, 2, PosType::Unicode).is_err());
}

#[test]
fn test_slice_delta_empty() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "A").unwrap();
    let delta = text.slice_delta(0, 0, PosType::Unicode).unwrap();
    assert!(delta.is_empty());
}

#[test]
fn test_slice_delta_utf16_positions() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    let content = "A😀BC💡";
    text.insert(0, content).unwrap();
    let char_len = content.chars().count();
    text.mark(0..2, "bold", true).unwrap(); // A and 😀
    text.mark(4..char_len, "bold", true).unwrap(); // 💡 outside of slice
    text.mark(1..3, "underline", true).unwrap(); // 😀 and B

    let start = utf16_pos(content, 1); // start at 😀 which takes 2 UTF-16 units
    let end = utf16_pos(content, 4); // end right before 💡
    let delta = text.slice_delta(start, end, PosType::Utf16).unwrap();
    assert_eq!(delta.len(), 3);

    if let TextDelta::Insert { insert, attributes } = &delta[0] {
        assert_eq!(insert, "😀");
        let attrs = attributes.as_ref().expect("attributes expected for emoji");
        assert_eq!(attrs.get("bold").unwrap(), &true.into());
        assert_eq!(attrs.get("underline").unwrap(), &true.into());
        assert_eq!(attrs.len(), 2);
    } else {
        panic!("Expected emoji segment");
    }

    if let TextDelta::Insert { insert, attributes } = &delta[1] {
        assert_eq!(insert, "B");
        let attrs = attributes.as_ref().expect("underline expected on 'B'");
        assert!(attrs.get("bold").is_none());
        assert_eq!(attrs.get("underline").unwrap(), &true.into());
    } else {
        panic!("Expected 'B' segment");
    }

    if let TextDelta::Insert { insert, attributes } = &delta[2] {
        assert_eq!(insert, "C");
        assert!(attributes.is_none(), "C should not carry attributes");
    } else {
        panic!("Expected 'C' segment");
    }
}

#[test]
fn utf16_insert_delete_and_slice() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "A😀C").unwrap();

    text.insert_utf16(1, "B").unwrap();
    assert_eq!(text.to_string(), "AB😀C");

    let current = text.to_string();
    let emoji_start = utf16_pos(&current, 2);
    text.delete_utf16(emoji_start, 2).unwrap();
    assert_eq!(text.to_string(), "ABC");

    let tail = text.slice_utf16(1, text.len_utf16()).unwrap();
    assert_eq!(tail, "BC");
}

#[test]
fn mark_and_unmark_utf16_ranges() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    let content = "A😀BC";
    text.insert(0, content).unwrap();

    let start = utf16_pos(content, 1);
    let end = utf16_pos(content, 3);
    text.mark_utf16(start..end, "bold", true).unwrap();

    let delta = text
        .slice_delta(0, text.len_unicode(), PosType::Unicode)
        .unwrap();
    assert_eq!(delta.len(), 3);

    if let TextDelta::Insert { insert, attributes } = &delta[0] {
        assert_eq!(insert, "A");
        assert!(attributes.is_none());
    } else {
        panic!("Expected leading segment");
    }

    if let TextDelta::Insert { insert, attributes } = &delta[1] {
        assert_eq!(insert, "😀B");
        let attrs = attributes.as_ref().expect("bold attribute expected");
        assert_eq!(attrs.get("bold"), Some(&true.into()));
    } else {
        panic!("Expected middle segment");
    }

    if let TextDelta::Insert { insert, attributes } = &delta[2] {
        assert_eq!(insert, "C");
        assert!(attributes.is_none());
    } else {
        panic!("Expected trailing segment");
    }

    text.unmark_utf16(start..end, "bold").unwrap();
    let delta = text
        .slice_delta(0, text.len_unicode(), PosType::Unicode)
        .unwrap();
    let mut combined = String::new();
    for segment in &delta {
        if let TextDelta::Insert { insert, attributes } = segment {
            combined.push_str(insert);
            if let Some(attrs) = attributes {
                if let Some(v) = attrs.get("bold") {
                    assert_ne!(
                        v,
                        &true.into(),
                        "expected formatting cleared, got {:?}",
                        attrs
                    );
                }
            }
        } else {
            panic!("Expected insert segment");
        }
    }
    assert_eq!(combined, content);
}

#[test]
fn convert_pos_across_coord_systems() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    let content = "A😀BC";
    text.insert(0, content).unwrap();

    // Unicode -> UTF-16
    assert_eq!(
        text.convert_pos(0, PosType::Unicode, PosType::Utf16),
        Some(0)
    );
    assert_eq!(
        text.convert_pos(1, PosType::Unicode, PosType::Utf16),
        Some(1)
    ); // after 'A'
    assert_eq!(
        text.convert_pos(2, PosType::Unicode, PosType::Utf16),
        Some(3)
    ); // after emoji (2 code units)

    // UTF-16 -> Unicode
    assert_eq!(
        text.convert_pos(3, PosType::Utf16, PosType::Unicode),
        Some(2)
    );

    // Unicode -> Bytes
    let utf8_len_before_emoji = "A".as_bytes().len();
    assert_eq!(
        text.convert_pos(1, PosType::Unicode, PosType::Bytes),
        Some(utf8_len_before_emoji)
    );

    // Out of bounds yields None
    assert_eq!(
        text.convert_pos(10, PosType::Unicode, PosType::Utf16),
        None
    );
}

#[test]
fn test_slice_delta_bytes_with_mixed_attributes() {
    let doc = LoroDoc::new();
    let mut styles = StyleConfigMap::default_rich_text_config();
    styles.insert(
        "script".into(),
        StyleConfig {
            expand: ExpandType::After,
        },
    );
    doc.config_text_style(styles);
    let text = doc.get_text("text");
    let content = "Rä😀汉字Z";
    text.insert(0, content).unwrap();
    let char_len = content.chars().count();
    text.mark(0..3, "bold", true).unwrap(); // R, ä, 😀
    text.mark(4..char_len, "bold", true).unwrap(); // 字 and beyond
    text.mark(2..4, "script", true).unwrap(); // 😀 and 汉

    let start = byte_pos(content, 1); // begin at 'ä' which is multi-byte
    let end = byte_pos(content, 5); // stop before the trailing 'Z'
    let delta = text.slice_delta(start, end, PosType::Bytes).unwrap();
    assert_eq!(delta.len(), 4);

    if let TextDelta::Insert { insert, attributes } = &delta[0] {
        assert_eq!(insert, "ä");
        let attrs = attributes.as_ref().expect("bold expected on 'ä'");
        assert_eq!(attrs.get("bold").unwrap(), &true.into());
        assert_eq!(attrs.len(), 1);
    } else {
        panic!("Expected 'ä' segment");
    }

    if let TextDelta::Insert { insert, attributes } = &delta[1] {
        assert_eq!(insert, "😀");
        let attrs = attributes.as_ref().expect("attributes expected on emoji");
        assert_eq!(attrs.get("bold").unwrap(), &true.into());
        assert_eq!(attrs.get("script").unwrap(), &true.into());
    } else {
        panic!("Expected emoji segment");
    }

    if let TextDelta::Insert { insert, attributes } = &delta[2] {
        assert_eq!(insert, "汉");
        let attrs = attributes.as_ref().expect("script expected on 汉");
        assert!(attrs.get("bold").is_none());
        assert_eq!(attrs.get("script").unwrap(), &true.into());
        assert_eq!(attrs.len(), 1);
    } else {
        panic!("Expected '汉' segment");
    }

    if let TextDelta::Insert { insert, attributes } = &delta[3] {
        assert_eq!(insert, "字");
        let attrs = attributes.as_ref().expect("bold expected on 字");
        assert_eq!(attrs.get("bold").unwrap(), &true.into());
        assert!(attrs.get("script").is_none());
    } else {
        panic!("Expected '字' segment");
    }
}
