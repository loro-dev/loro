#![allow(unexpected_cfgs)]
use std::collections::HashMap;

use loro_delta::{
    text_delta::{TextChunk, TextDelta},
    DeltaRopeBuilder,
};
use tracing_subscriber::fmt::format::FmtSpan;

#[ctor::ctor]
fn init_color_backtrace() {
    color_backtrace::install();
    use tracing_subscriber::{prelude::*, registry::Registry};
    if option_env!("DEBUG").is_some() {
        tracing::subscriber::set_global_default(
            Registry::default().with(
                tracing_subscriber::fmt::Layer::default()
                    .with_file(true)
                    .with_line_number(true)
                    .with_span_events(FmtSpan::ACTIVE),
            ),
        )
        .unwrap();
    }
}

#[test]
fn long_text_insert_preserves_all_bytes() {
    for n in [127, 128, 129, 255, 256, 257, 1025] {
        let input: String = (0..n).map(|i| (b'a' + (i % 26) as u8) as char).collect();
        let mut text: TextDelta = TextDelta::new();
        text.push_str_insert(&input);
        assert_eq!(text.len(), input.len(), "input length {n}");
        assert_eq!(text.try_to_string().unwrap(), input);

        text.insert_str(text.len(), &input);
        assert_eq!(text.try_to_string().unwrap(), input.repeat(2));
    }
}

#[test]
fn long_utf8_insert_preserves_characters() {
    for character in ["é", "中", "🦀"] {
        let input = format!("{}{}", "x".repeat(127), character.repeat(130));
        let mut text: TextDelta = TextDelta::new();
        text.push_str_insert(&input);
        assert_eq!(text.try_to_string().unwrap(), input);

        let mut text: TextDelta = TextDelta::new();
        text.push_str_insert("ab");
        text.insert_str(1, &input);
        assert_eq!(text.try_to_string().unwrap(), format!("a{input}b"));
    }
}

#[test]
fn long_text_insert_preserves_attributes() {
    let input = "aé中🦀".repeat(100);
    let attrs = HashMap::from([("bold".to_string(), true)]);
    let mut text = TextDelta::new();
    text.push_str_insert_with_attr(&input, attrs.clone());
    assert_eq!(text.try_to_string().unwrap(), input);
    for item in text.iter() {
        let loro_delta::DeltaItem::Replace { attr, .. } = item else {
            panic!("expected an insert");
        };
        assert_eq!(attr, &attrs);
    }
}

#[test]
fn utf8_middle_insert_preserves_order_and_existing_attributes() {
    let input = format!("{}é", "x".repeat(127));
    let bold = HashMap::from([("bold".to_string(), true)]);
    let mut text = TextDelta::new();
    text.push_str_insert("a");
    text.push_str_insert_with_attr("bc", bold.clone());
    text.insert_str(2, &input);
    assert_eq!(text.try_to_string().unwrap(), format!("ab{input}c"));

    let mut expected = TextDelta::new();
    expected.push_str_insert("a");
    expected.push_str_insert_with_attr("b", bold.clone());
    expected.push_str_insert(&input);
    expected.push_str_insert_with_attr("c", bold);
    assert_eq!(text, expected);

    text.push_str_insert("after");
    assert_eq!(text.try_to_string().unwrap(), format!("ab{input}cafter"));
}

#[test]
fn repeated_utf8_middle_insert_matches_string() {
    let mut text: TextDelta = TextDelta::new();
    let mut expected = String::new();
    for i in 0..1_000 {
        let input = format!("{}é中🦀", "x".repeat(i % 258));
        let pos = expected
            .char_indices()
            .nth(i % (expected.chars().count() + 1))
            .map_or(expected.len(), |(pos, _)| pos);
        text.insert_str(pos, &input);
        expected.insert_str(pos, &input);
        assert_eq!(text.try_to_string().unwrap(), expected, "edit {i}");
        assert_eq!(text.len(), expected.len());
    }
}

#[test]
fn text_delta() {
    let mut text = TextDelta::new();
    text.push_str_insert("123456789");
    assert_eq!(text.try_to_string().unwrap(), "123456789");
    let mut delta = TextDelta::new();
    delta
        .push_str_insert("abc")
        .push_retain(3, ())
        .push_delete(3);
    text.compose(&delta);
    assert_eq!(text.try_to_string().unwrap(), "abc123789");
}

#[test]
fn delete_delta_compose() {
    let mut a: TextDelta = DeltaRopeBuilder::new().delete(5).build();
    let b: TextDelta = DeltaRopeBuilder::new().delete(5).build();
    a.compose(&b);
    assert_eq!(a, DeltaRopeBuilder::new().delete(10).build());
}

#[test]
fn insert_long() {
    let mut a: TextDelta = TextDelta::new();
    a.push_str_insert("1234567890");
    a.insert_str(3, &"abc".repeat(10));
    assert_eq!(
        a,
        DeltaRopeBuilder::new()
            .insert(TextChunk::try_from_str("123abc").unwrap(), ())
            .insert(TextChunk::try_from_str("abcabcabc").unwrap(), ())
            .insert(TextChunk::try_from_str("abcabcabc").unwrap(), ())
            .insert(TextChunk::try_from_str("abcabcabc").unwrap(), ())
            .insert(TextChunk::try_from_str("4567890").unwrap(), ())
            .build()
    );
}

#[test]
fn retain_delete_delta_compose() {
    let mut a: TextDelta = DeltaRopeBuilder::new().retain(5, ()).build();
    let b: TextDelta = DeltaRopeBuilder::new().delete(5).build();
    a.compose(&b);
    assert_eq!(a, DeltaRopeBuilder::new().delete(5).build());
}

#[test]
fn retain_delete_delta_compose_1() {
    let mut a: TextDelta = DeltaRopeBuilder::new().retain(10, ()).build();
    let b: TextDelta = DeltaRopeBuilder::new().retain(2, ()).delete(5).build();
    a.compose(&b);
    assert_eq!(
        a,
        DeltaRopeBuilder::new()
            .retain(2, ())
            .delete(5)
            .retain(3, ())
            .build()
    );
}

#[test]
fn compose_long_delete() {
    let mut a: TextDelta = TextDelta::new();
    a.push_retain(5, ());
    a.push_str_insert("1234567890");
    a.push_retain(5, ());
    a.push_delete(1);
    a.push_str_insert("1234567890");
    let b: TextDelta = DeltaRopeBuilder::new().retain(2, ()).delete(20).build();
    a.compose(&b);
    assert_eq!(
        a,
        DeltaRopeBuilder::new()
            .retain(2, ())
            .replace(TextChunk::try_from_str("34567890").unwrap(), (), 9)
            .build()
    );
}

type RichTextDelta = TextDelta<HashMap<String, bool>>;
#[test]
fn rich_text_delta() {
    let mut text = RichTextDelta::new();
    text.push_str_insert("123456789");
    assert_eq!(text.try_to_string().unwrap(), "123456789");
    let mut delta = RichTextDelta::new();
    let mut styles = HashMap::new();
    styles.insert("bold".to_string(), true);
    delta
        .push_str_insert("abc")
        .push_retain(3, styles.clone())
        .push_delete(3);
    text.compose(&delta);

    let mut expected = RichTextDelta::new();
    expected
        .push_str_insert("abc")
        .push_insert(TextChunk::try_from_str("123").unwrap(), styles.clone())
        .push_str_insert("789");

    assert_eq!(text, expected);
}

#[test]
fn insert_plus_insert() {
    let mut a: TextDelta = TextDelta::new();
    a.push_str_insert("A");
    let mut b = TextDelta::new();
    b.push_str_insert("B");
    let expected = {
        let mut delta = TextDelta::new();
        delta.push_str_insert("B").push_str_insert("A");
        delta
    };
    a.compose(&b);
    assert_eq!(a, expected);
}

#[test]
fn insert_plus_retain() {
    let mut a: RichTextDelta = TextDelta::new();
    a.push_str_insert("A");
    let mut b: RichTextDelta = TextDelta::new();
    let mut attrs = HashMap::new();
    attrs.insert("bold".to_string(), true);
    attrs.insert("color".to_string(), true);
    b.push_retain(1, attrs.clone());
    let expected = {
        let mut delta = TextDelta::new();
        delta.push_insert(TextChunk::try_from_str("A").unwrap(), attrs);
        delta
    };
    a.compose(&b);
    assert_eq!(a, expected);
}

#[test]
fn insert_plus_delete() {
    let mut a: TextDelta = TextDelta::new();
    a.push_str_insert("A");
    let mut b: TextDelta = TextDelta::new();
    b.push_delete(1);
    let expected = TextDelta::new();
    a.compose(&b);
    assert_eq!(a, expected);
}

#[test]
fn delete_plus_delete() {
    let mut a: TextDelta = TextDelta::new();
    a.push_delete(1);
    let mut b: TextDelta = TextDelta::new();
    b.push_delete(1);
    let expected = {
        let mut delta = TextDelta::new();
        delta.push_delete(2);
        delta
    };
    a.compose(&b);
    assert_eq!(a, expected);
}

#[test]
fn retain_plus_insert() {
    let mut a: RichTextDelta = TextDelta::new();
    let mut attrs = HashMap::new();
    attrs.insert("color".to_string(), true);
    a.push_retain(1, attrs.clone());
    let mut b = TextDelta::new();
    b.push_str_insert("B");
    let expected = {
        let mut delta = TextDelta::new();
        delta.push_str_insert("B").push_retain(1, attrs);
        delta
    };
    a.compose(&b);
    assert_eq!(a, expected);
}

#[test]
fn retain_plus_retain() {
    let mut a: RichTextDelta = TextDelta::new();
    let mut attrs_a = HashMap::new();
    attrs_a.insert("color".to_string(), true);
    a.push_retain(1, attrs_a.clone());
    let mut b = TextDelta::new();
    let mut attrs_b = HashMap::new();
    attrs_b.insert("bold".to_string(), true);
    attrs_b.insert("color".to_string(), true);
    b.push_retain(1, attrs_b.clone());
    let expected = {
        let mut delta = TextDelta::new();
        delta.push_retain(1, attrs_b);
        delta
    };
    a.compose(&b);
    assert_eq!(a, expected);
}

#[test]
fn retain_plus_delete() {
    let mut a: RichTextDelta = TextDelta::new();
    let mut attrs = HashMap::new();
    attrs.insert("color".to_string(), true);
    a.push_retain(1, attrs);
    let mut b = TextDelta::new();
    b.push_delete(1);
    let mut expected: RichTextDelta = TextDelta::new();
    expected.push_delete(1);
    a.compose(&b);
    assert_eq!(a, expected);
}

// Test for inserting in the middle of text
#[test]
fn insert_in_middle_of_text() {
    let mut a: TextDelta = TextDelta::new();
    a.push_str_insert("Hello");
    let mut b: TextDelta = TextDelta::new();
    b.push_retain(3, ()).push_str_insert("X");
    let mut expected: TextDelta = TextDelta::new();
    expected.push_str_insert("HelXlo");
    a.compose(&b);
    assert_eq!(a, expected);
}

// Test for insert and delete ordering
#[test]
fn insert_and_delete_ordering() {
    let mut a: TextDelta = TextDelta::new();
    a.push_str_insert("Hello");
    let mut insert_first: TextDelta = TextDelta::new();
    insert_first
        .push_retain(3, ())
        .push_str_insert("X")
        .push_delete(1);
    let mut delete_first = TextDelta::new();
    delete_first
        .push_retain(3, ())
        .push_delete(1)
        .push_str_insert("X");
    let mut expected: TextDelta = TextDelta::new();
    expected.push_str_insert("HelXo");
    a.compose(&insert_first);
    a.compose(&delete_first);
    assert_eq!(a, expected);
}

#[test]
fn retain_start_optimization_split() {
    let mut a: RichTextDelta = TextDelta::new();
    let mut attrs_bold = HashMap::new();
    attrs_bold.insert("bold".to_string(), true);

    a.push_insert(TextChunk::try_from_str("A").unwrap(), attrs_bold.clone())
        .push_str_insert("B")
        .push_insert(TextChunk::try_from_str("C").unwrap(), attrs_bold)
        .push_retain(5, Default::default())
        .push_delete(1);

    let mut b: RichTextDelta = TextDelta::new();
    b.push_retain(4, Default::default()).push_str_insert("D");

    let expected = {
        let mut delta = TextDelta::new();
        let mut attrs_bold = HashMap::new();
        attrs_bold.insert("bold".to_string(), true);

        delta
            .push_insert(TextChunk::try_from_str("A").unwrap(), attrs_bold.clone())
            .push_str_insert("B")
            .push_insert(TextChunk::try_from_str("C").unwrap(), attrs_bold)
            .push_retain(1, Default::default())
            .push_str_insert("D")
            .push_retain(4, Default::default())
            .push_delete(1);
        delta
    };

    a.compose(&b);
    assert_eq!(a, expected);
}
