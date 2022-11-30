use std::time::Instant;

use std::cell::RefCell;
use std::rc::Rc;

use ctor::ctor;

use loro_core::container::registry::ContainerWrapper;
use loro_core::{ContainerType, LoroCore, LoroValue, VersionVector};

#[test]
#[cfg(feature = "json")]
fn example() {
    use loro_core::ContainerType;

    let mut doc = LoroCore::default();
    let mut list = doc.get_list("list");
    list.insert(&doc, 0, 123).unwrap();
    let map_id = list.insert(&doc, 1, ContainerType::Map).unwrap().unwrap();
    let mut map = doc.get_map(map_id);
    let text = map
        .insert(&doc, "map_b", ContainerType::Text)
        .unwrap()
        .unwrap();
    let mut text = doc.get_text(text);
    text.insert(&doc, 0, "world!").unwrap();
    text.insert(&doc, 0, "hello ").unwrap();
    assert_eq!(
        r#"[123,{"map_b":"hello world!"}]"#,
        list.get_value_deep(&doc).to_json()
    );
}

#[test]
#[cfg(feature = "json")]
fn text_observe() {
    let mut doc = LoroCore::default();
    let track_value = Rc::new(RefCell::new(LoroValue::Map(Default::default())));
    let moved_value = Rc::clone(&track_value);
    doc.subscribe_deep(Box::new(move |event| {
        let mut v = RefCell::borrow_mut(&*moved_value);
        v.apply(&event.relative_path, &event.diff);
    }));
    let mut map = doc.get_map("meta");
    map.insert(&doc, "name", "anonymous").unwrap();
    let list = map
        .insert(&doc, "to-dos", ContainerType::List)
        .unwrap()
        .unwrap();
    let mut list = doc.get_list(list);
    let todo_item = list.insert(&doc, 0, ContainerType::Map).unwrap().unwrap();
    let mut todo_item = doc.get_map(todo_item);
    todo_item.insert(&doc, "todo", "coding").unwrap();
    assert_eq!(&doc.to_json(), &*RefCell::borrow(&track_value));
    let mut text = doc.get_text("text");
    text.insert(&doc, 0, "hello ").unwrap();
    let mut doc_b = LoroCore::default();
    let mut text_b = doc_b.get_text("text");
    text_b.insert(&doc_b, 0, "world").unwrap();
    doc.import(doc_b.export(Default::default()));
    assert_eq!(&doc.to_json(), &*RefCell::borrow(&track_value));
    println!("{}", doc.to_json().to_json());
}

#[test]
#[cfg(feature = "json")]
fn list() {
    let mut loro_a = LoroCore::default();
    let mut loro_b = LoroCore::default();
    let mut list_a = loro_a.get_list("list");
    let mut list_b = loro_b.get_list("list");
    list_a
        .insert_batch(&loro_a, 0, vec![12.into(), "haha".into()])
        .unwrap();
    list_b
        .insert_batch(&loro_b, 0, vec![123.into(), "kk".into()])
        .unwrap();
    let map_id = list_b
        .insert(&loro_b, 1, loro_core::ContainerType::Map)
        .unwrap()
        .unwrap();
    let mut map = loro_b.get_map(map_id);
    map.insert(&loro_b, "map_b", 123).unwrap();
    println!("{}", list_a.get_value().to_json());
    println!("{}", list_b.get_value().to_json());
    loro_b.import(loro_a.export(loro_b.vv()));
    loro_a.import(loro_b.export(loro_a.vv()));
    println!("{}", list_b.get_value_deep(&loro_b).to_json());
    println!("{}", list_a.get_value_deep(&loro_b).to_json());
    assert_eq!(list_b.get_value(), list_a.get_value());
}

#[test]
#[cfg(feature = "json")]
fn map() {
    let mut loro = LoroCore::new(Default::default(), Some(10));
    let mut root = loro.get_map("root");
    root.insert(&loro, "haha", 1.2).unwrap();
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
    let map_id = root
        .insert(&loro, "map", loro_core::ContainerType::Map)
        .unwrap()
        .unwrap();
    drop(root);
    let mut sub_map = loro.get_map(&map_id);
    sub_map.insert(&loro, "sub", false).unwrap();
    drop(sub_map);
    let root = loro.get_map("root");
    let value = root.get_value();
    assert_eq!(value.as_map().unwrap().len(), 2);
    let map = value.as_map().unwrap();
    assert_eq!(*map.get("haha").unwrap().as_double().unwrap(), 1.2);
    assert!(map.get("map").unwrap().as_unresolved().is_some());
    println!("{}", value.to_json());
    let deep_value = root.get_value_deep(&loro);
    assert_eq!(deep_value.as_map().unwrap().len(), 2);
    let map = deep_value.as_map().unwrap();
    assert_eq!(*map.get("haha").unwrap().as_double().unwrap(), 1.2);
    let inner_map = map.get("map").unwrap().as_map().unwrap();
    assert_eq!(inner_map.len(), 1);
    assert_eq!(inner_map.get("sub").unwrap(), &LoroValue::Bool(false));
    let json = deep_value.to_json();
    // println!("{}", json);
    let actual: LoroValue = serde_json::from_str(&json).unwrap();
    // dbg!(&actual);
    assert_eq!(actual, deep_value);
}

#[test]
fn two_client_text_sync() {
    let mut store = LoroCore::new(Default::default(), Some(10));
    let mut text_container = store.get_text("haha");
    text_container.insert(&store, 0, "012").unwrap();
    text_container.insert(&store, 1, "34").unwrap();
    text_container.insert(&store, 1, "56").unwrap();
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(&**value, "0563412");
    drop(text_container);

    let mut store_b = LoroCore::new(Default::default(), Some(11));
    let exported = store.export(Default::default());
    store_b.import(exported);
    let mut text_container = store_b.get_text("haha");
    text_container.with_container(|x| x.check());
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(&**value, "0563412");

    text_container.delete(&store_b, 0, 2).unwrap();
    text_container.insert(&store_b, 4, "789").unwrap();
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(&**value, "63417892");
    drop(text_container);

    store.import(store_b.export(store.vv()));
    let mut text_container = store.get_text("haha");
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(&**value, "63417892");
    text_container.delete(&store, 0, 8).unwrap();
    text_container.insert(&store, 0, "abc").unwrap();
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(&**value, "abc");

    store_b.import(store.export(Default::default()));
    let text_container = store_b.get_text("haha");
    text_container.with_container(|x| x.check());
    let value = text_container.get_value();
    let value = value.as_string().unwrap();
    assert_eq!(&**value, "abc");
}

#[test]
#[should_panic]
fn test_recursive_should_panic() {
    let mut store_a = LoroCore::new(Default::default(), Some(1));
    let mut store_b = LoroCore::new(Default::default(), Some(2));
    let mut text_a = store_a.get_text("text_a");
    let mut text_b = store_b.get_text("text_b");
    text_a.insert(&store_a, 0, "012").unwrap();
    text_b.insert(&store_a, 1, "34").unwrap();
}

#[test]
#[cfg(feature = "json")]
fn test_encode_state() {
    let mut store = LoroCore::new(Default::default(), Some(1));
    let mut list = store.get_list("list");
    list.insert(&store, 0, "some thing").unwrap();
    list.insert(&store, 1, "some thing").unwrap();
    list.insert(&store, 2, "some thing").unwrap();
    list.insert(&store, 3, "some thing else").unwrap();
    let id = list
        .insert(&store, 4, ContainerType::List)
        .unwrap()
        .unwrap();
    let mut list2 = store.get_list(id);
    list2.insert(&store, 0, "some hahaha").unwrap();
    let start = Instant::now();
    let buf = store.encode_snapshot(&VersionVector::new());
    println!(
        "size: {:?} bytes time: {} ms",
        buf.len(),
        start.elapsed().as_millis()
    );
    let start = Instant::now();
    let mut store2 = LoroCore::new(Default::default(), Some(2));

    store2
        .get_text("text")
        .insert(&store2, 0, "some text")
        .unwrap();

    store2.decode_snapshot(&buf);
    println!("############\n\n");
    let buf2 = store2.encode_snapshot(&VersionVector::new());
    store.decode_snapshot(&buf2);
    println!("decode time: {} ms", start.elapsed().as_millis());
    println!("store: {}", store.to_json().to_json_pretty());
    println!("store2: {}", store2.to_json().to_json_pretty());
    assert_eq!(store.to_json(), store2.to_json());
    // let buf2 = store2.encode_snapshot(&VersionVector::new());
    // assert_eq!(buf, buf2);
}

#[test]
fn test_encode_state_text() {
    let mut store = LoroCore::new(Default::default(), Some(1));
    let mut text = store.get_text("text");
    for _ in 0..1000 {
        text.insert(&store, 0, "some thing").unwrap();
    }
    text.insert(&store, 0, "some thing").unwrap();
    text.delete(&store, 2, 10).unwrap();
    text.delete(&store, 4, 12).unwrap();
    let start = Instant::now();
    let buf = store.encode_snapshot(&VersionVector::new());
    println!(
        "size: {:?} bytes time: {} ms",
        buf.len(),
        start.elapsed().as_millis()
    );
    let start = Instant::now();
    let mut store2 = LoroCore::new(Default::default(), Some(1));
    store2.decode_snapshot(&buf);
    println!("decode time: {} ms", start.elapsed().as_millis());
    assert_eq!(store.to_json(), store2.to_json());
    let buf2 = store2.encode_snapshot(&VersionVector::new());
    assert_eq!(buf, buf2);
}

#[test]
fn test_encode_state_map() {
    let mut store = LoroCore::new(Default::default(), Some(1));
    let mut map = store.get_map("map");
    map.insert(&store, "aa", "some thing").unwrap();
    map.insert(&store, "bb", 10).unwrap();
    map.insert(&store, "cc", 12).unwrap();
    map.delete(&store, "cc").unwrap();
    let start = Instant::now();
    let buf = store.encode_snapshot(&VersionVector::new());
    println!(
        "size: {:?} bytes time: {} ms",
        buf.len(),
        start.elapsed().as_millis()
    );
    let mut store2 = LoroCore::new(Default::default(), Some(1));
    store2.decode_snapshot(&buf);
    println!("store2: {}", store.to_json().to_json_pretty());
    println!("store2: {}", store2.to_json().to_json_pretty());
    assert_eq!(store.to_json(), store2.to_json());
    let buf2 = store2.encode_snapshot(&VersionVector::new());
    assert_eq!(buf, buf2);
}

#[ctor]
fn init_color_backtrace() {
    color_backtrace::install();
}
