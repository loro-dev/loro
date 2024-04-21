use std::collections::HashMap;

use delta::{
    text_delta::{Chunk, TextDelta},
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
            .delete(1)
            .insert(Chunk::try_from_str("34567890").unwrap(), ())
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
        .push_insert(Chunk::try_from_str("123").unwrap(), styles.clone())
        .push_str_insert("789");

    assert_eq!(text, expected);
}
