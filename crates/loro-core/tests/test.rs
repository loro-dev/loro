use ctor::ctor;
use loro_core::container::Container;
use loro_core::{InsertValue, LoroCore};

#[test]
fn map() {
    let mut loro = LoroCore::new(Default::default(), Some(10));
    let mut root = loro.get_or_create_root_map("root").unwrap();
    root.insert("haha".into(), InsertValue::Double(1.2));
}

#[test]
fn two_client_text_sync() {
    let mut store = LoroCore::new(Default::default(), Some(10));
    let mut text_container = store.get_or_create_root_text("haha").unwrap();
    text_container.insert(0, "012");
    text_container.insert(1, "34");
    text_container.insert(1, "56");
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(value.as_str(), "0563412");
    drop(text_container);

    let mut store_b = LoroCore::new(Default::default(), Some(11));
    let exported = store.export(Default::default());
    store_b.import(exported);
    let mut text_container = store_b.get_or_create_root_text("haha").unwrap();
    text_container.check();
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(value.as_str(), "0563412");

    text_container.delete(0, 2);
    text_container.insert(4, "789");
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(value.as_str(), "63417892");
    drop(text_container);

    store.import(store_b.export(store.vv()));
    let mut text_container = store.get_or_create_root_text("haha").unwrap();
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(value.as_str(), "63417892");
    text_container.delete(0, 8);
    text_container.insert(0, "abc");
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(value.as_str(), "abc");
    drop(text_container);

    store_b.import(store.export(Default::default()));
    let mut text_container = store_b.get_or_create_root_text("haha").unwrap();
    text_container.check();
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(value.as_str(), "abc");
}

#[ctor]
fn init_color_backtrace() {
    color_backtrace::install();
}
