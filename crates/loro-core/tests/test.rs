use ctor::ctor;
use loro_core::container::Container;
use loro_core::LoroCore;

#[ignore]
#[test]
fn test() {
    let mut store = LoroCore::new(Default::default(), Some(10));
    let mut text_container = store.get_text_container("haha".into());
    text_container.insert(0, "abc");
    text_container.insert(1, "xx");
    text_container.insert(1, "vv");
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(value.as_str(), "avvxxbc");
    drop(text_container);
    let mut store_b = LoroCore::new(Default::default(), Some(10));
    store_b.import(store.export(Default::default()));
    let mut text_container = store_b.get_text_container("haha".into());
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(value.as_str(), "avvxxbc");
}

#[ctor]
fn init_color_backtrace() {
    color_backtrace::install();
}
