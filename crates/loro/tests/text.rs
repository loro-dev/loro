use loro::{LoroDoc, TextDelta};
use loro::cursor::PosType;

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