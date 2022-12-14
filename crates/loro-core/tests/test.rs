use std::cell::RefCell;
use std::rc::Rc;

use ctor::ctor;

use loro_core::container::registry::ContainerWrapper;
use loro_core::context::Context;
use loro_core::{ContainerType, LoroCore, LoroValue};

#[test]
fn example_list() {
    let mut doc = LoroCore::default();
    let mut list = doc.get_list("list");
    list.insert(&doc, 0, 11).unwrap();
    list.insert(&doc, 1, 22).unwrap();
    dbg!(&doc.log_store());
}

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
fn subscribe_deep() {
    let mut doc = LoroCore::default();
    doc.subscribe_deep(Box::new(move |event| {
        println!("event: {:?}", event);
    }));
    let mut text = doc.get_text("root");
    text.insert(&doc, 0, "abc").unwrap();
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
    loro_b.import(loro_a.export(loro_b.vv_cloned()));
    loro_a.import(loro_b.export(loro_a.vv_cloned()));
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

    store.import(store_b.export(store.vv_cloned()));
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
fn test_to_json() {
    let mut loro = LoroCore::new(Default::default(), Some(10));
    let mut map = loro.get_map("A map");
    map.insert(&loro, "haha", 1.2).unwrap();
    let a = map
        .insert(&loro, "text container", ContainerType::Text)
        .unwrap()
        .unwrap();
    let mut text = loro.get_text(&a);
    text.insert(&loro, 0, "012").unwrap();
    println!("{}", loro.to_json().to_json());
}

#[ctor]
fn init_color_backtrace() {
    color_backtrace::install();
}
