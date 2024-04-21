use delta::{text_delta::TextDelta, DeltaRopeBuilder};

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
