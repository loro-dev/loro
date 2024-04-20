use delta::text_delta::TextDelta;

#[test]
fn text_delta() {
    let mut text = TextDelta::new();
    text.push_str_insert("123456789");
    assert_eq!(text.try_to_string().unwrap(), "123456789");
    let mut delta = TextDelta::new();
    delta.push_str_insert("abc").new_retain(3, ()).new_delete(3);
    text.compose(&delta);
    assert_eq!(text.try_to_string().unwrap(), "abc123789");
}
