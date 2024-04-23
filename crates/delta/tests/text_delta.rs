use std::collections::HashMap;

use loro_delta::{
    text_delta::{TextChunk, TextDelta},
    DeltaRopeBuilder,
};

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
