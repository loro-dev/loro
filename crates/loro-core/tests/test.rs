use ctor::ctor;
use fxhash::FxHashMap;
use loro_core::container::manager::LockContainer;
use loro_core::container::Container;
use loro_core::{InsertValue, LoroCore};

#[test]
fn map() {
    let mut loro = LoroCore::new(Default::default(), Some(10));
    let get_or_create_root_map = loro.get_or_create_root_map("root");
    let mut root = get_or_create_root_map.lock_map();
    root.insert("haha".into(), InsertValue::Double(1.2));
    let value = root.get_value();
    assert_eq!(value.as_map().unwrap().len(), 1);
    assert_eq!(
        *value
            .as_map()
            .unwrap()
            .get("haha")
            .unwrap()
            .as_double()
            .unwrap(),
        1.2
    );
    let map_id = root.insert_obj("map".into(), loro_core::ContainerType::Map);
    drop(root);
    let arc = loro.get_container(&map_id).unwrap();
    let mut sub_map = arc.lock_map();
    sub_map.insert("sub".into(), InsertValue::Bool(false));
    drop(sub_map);
    let get_or_create_root_map = loro.get_or_create_root_map("root");
    let root = get_or_create_root_map.lock_map();
    let value = root.get_value();
    assert_eq!(value.as_map().unwrap().len(), 2);
    let map = value.as_map().unwrap();
    assert_eq!(*map.get("haha").unwrap().as_double().unwrap(), 1.2);
    let mut expected_map: FxHashMap<String, _> = FxHashMap::default();
    expected_map.insert("sub".into(), loro_core::LoroValue::Bool(false));
    assert_eq!(**map.get("map").unwrap().as_map().unwrap(), expected_map);
}

#[test]
fn two_client_text_sync() {
    let mut store = LoroCore::new(Default::default(), Some(10));
    let get_or_create_root_text = store.get_or_create_root_text("haha");
    let mut text_container = get_or_create_root_text.lock_text();
    text_container.insert(0, "012");
    text_container.insert(1, "34");
    text_container.insert(1, "56");
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(&**value, "0563412");
    drop(text_container);

    let mut store_b = LoroCore::new(Default::default(), Some(11));
    let exported = store.export(Default::default());
    store_b.import(exported);
    let get_or_create_root_text = store_b.get_or_create_root_text("haha");
    let mut text_container = get_or_create_root_text.lock_text();
    text_container.check();
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(&**value, "0563412");

    text_container.delete(0, 2);
    text_container.insert(4, "789");
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(&**value, "63417892");
    drop(text_container);

    store.import(store_b.export(store.vv()));
    let get_or_create_root_text = store.get_or_create_root_text("haha");
    let mut text_container = get_or_create_root_text.lock_text();
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(&**value, "63417892");
    text_container.delete(0, 8);
    text_container.insert(0, "abc");
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(&**value, "abc");
    drop(text_container);

    store_b.import(store.export(Default::default()));
    let get_or_create_root_text = store_b.get_or_create_root_text("haha");
    let mut text_container = get_or_create_root_text.lock_text();
    text_container.check();
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(&**value, "abc");
}

#[ctor]
fn init_color_backtrace() {
    color_backtrace::install();
}
