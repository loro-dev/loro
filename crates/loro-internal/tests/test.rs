use ctor::ctor;

use loro_internal::container::registry::ContainerWrapper;
use loro_internal::container::ContainerID;
use loro_internal::context::Context;
use loro_internal::id::ID;

use loro_internal::{ContainerType, EncodeMode, LoroCore, Text, VersionVector};

#[test]
fn send_sync() {
    fn example<T: Send + Sync + 'static>(_: T) {}
    let loro = LoroCore::default();
    example(loro);
}

#[test]
fn example_list() {
    let mut doc = LoroCore::default();
    let mut list = doc.get_list("list");
    list.insert(&doc, 0, 11).unwrap();
    list.insert(&doc, 1, 22).unwrap();
    dbg!(&doc.log_store());
}

#[test]
fn text_utf16() {
    let mut doc = LoroCore::default();
    let mut text = doc.get_text("text");
    text.insert_utf16(&doc, 0, "你好").unwrap();
    text.insert_utf16(&doc, 1, "我").unwrap();
    text.insert_utf16(&doc, 1, "abc").unwrap();
    assert_eq!(text.get_value().as_string().unwrap().as_ref(), "你abc我好");
    text.delete_utf16(&doc, 1, 1).unwrap();
    assert_eq!(text.get_value().as_string().unwrap().as_ref(), "你bc我好");
    text.delete_utf16(&doc, 0, 1).unwrap();
    assert_eq!(text.get_value().as_string().unwrap().as_ref(), "bc我好");
}

#[test]
#[cfg(feature = "json")]
fn example() {
    use loro_internal::ContainerType;

    let mut doc = LoroCore::default();
    let mut list = doc.get_list("list");
    list.insert(&doc, 0, 123).unwrap();
    let map_id = list.insert(&doc, 1, ContainerType::Map).unwrap().unwrap();
    let mut map = doc.get_map_by_idx(&map_id).unwrap();
    let text = map
        .insert(&doc, "map_b", ContainerType::Text)
        .unwrap()
        .unwrap();
    let mut text = doc.get_text_by_idx(&text).unwrap();
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
    use std::sync::{Arc, Mutex};

    use loro_internal::LoroValue;

    let mut doc = LoroCore::default();
    let track_value = Arc::new(Mutex::new(LoroValue::Map(Default::default())));
    let moved_value = Arc::clone(&track_value);
    doc.subscribe_deep(Box::new(move |event| {
        let mut v = moved_value.lock().unwrap();
        v.apply(&event.relative_path, &event.diff);
    }));
    let mut map = doc.get_map("meta");
    map.insert(&doc, "name", "anonymous").unwrap();
    let list = map
        .insert(&doc, "to-dos", ContainerType::List)
        .unwrap()
        .unwrap();
    let mut list = doc.get_list_by_idx(&list).unwrap();
    let todo_item = list.insert(&doc, 0, ContainerType::Map).unwrap().unwrap();
    let mut todo_item = doc.get_map_by_idx(&todo_item).unwrap();
    todo_item.insert(&doc, "todo", "coding").unwrap();
    assert_eq!(&doc.to_json(), &*track_value.lock().unwrap());
    let mut text = doc.get_text("text");
    text.insert(&doc, 0, "hello ").unwrap();
    let mut doc_b = LoroCore::default();
    let mut text_b = doc_b.get_text("text");
    text_b.insert(&doc_b, 0, "world").unwrap();
    doc.import(doc_b.export(Default::default()));
    assert_eq!(&doc.to_json(), &*track_value.lock().unwrap());
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
        .insert(&loro_b, 1, loro_internal::ContainerType::Map)
        .unwrap()
        .unwrap();
    let mut map = loro_b.get_map_by_idx(&map_id).unwrap();
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
    use loro_internal::LoroValue;

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
        .insert(&loro, "map", loro_internal::ContainerType::Map)
        .unwrap()
        .unwrap();
    drop(root);
    let mut sub_map = loro.get_map_by_idx(&map_id).unwrap();
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
fn fix_fields_order() {
    // ContainerType ContainerID Index ID
    // TotalOrderStamp RemoteContent MapSet ListOp DeleteSpan ListSlice (mod test)
    let id = ID::new(0, 1);
    let id_buf = vec![0, 2];
    assert_eq!(postcard::from_bytes::<ID>(&id_buf).unwrap(), id);

    let container_type = vec![ContainerType::List, ContainerType::Map, ContainerType::Text];
    let container_type_buf = vec![3, 2, 1, 0];
    assert_eq!(
        postcard::from_bytes::<Vec<ContainerType>>(&container_type_buf).unwrap(),
        container_type
    );

    let container_id = vec![
        ContainerID::new_root("root", ContainerType::List),
        ContainerID::new_normal(ID::new(0, 0), ContainerType::Text),
    ];
    let container_id_buf = vec![2, 0, 4, 114, 111, 111, 116, 2, 1, 0, 0, 0];
    assert_eq!(
        postcard::from_bytes::<Vec<ContainerID>>(&container_id_buf).unwrap(),
        container_id
    );
}

#[test]
fn encode_hierarchy() {
    fn assert_eq(c1: &LoroCore, c2: &LoroCore) {
        let h1 = c1.hierarchy();
        let h1 = h1.try_lock().unwrap();

        let h2 = c2.hierarchy();
        let h2 = h2.try_lock().unwrap();
        assert_eq!(format!("{:?}", h1), format!("{:?}", h2));
        assert_eq!(c1.to_json(), c2.to_json());
    }

    let mut c1 = LoroCore::default();
    let mut map = c1.get_map("map");
    let list_id = map.insert(&c1, "a", ContainerType::List).unwrap().unwrap();
    let mut list = c1.get_list_by_idx(&list_id).unwrap();
    let idx = list.insert(&c1, 0, ContainerType::Text).unwrap().unwrap();
    let text = c1.get_container_by_idx(&idx).unwrap();
    let mut text = Text::from_instance(text, c1.client_id());
    text.insert(&c1, 0, "text_text").unwrap();

    // updates
    println!("updates");
    let input = c1.encode_with_cfg(EncodeMode::Updates(VersionVector::new()));
    let mut c2 = LoroCore::default();
    c2.subscribe_deep(Box::new(move |_event| {
        // println!("event: {:?}", _event);
    }));
    c2.decode(&input).unwrap();
    assert_eq(&c1, &c2);

    // rle updates
    println!("rle updates");
    let input = c1.encode_with_cfg(EncodeMode::RleUpdates(VersionVector::new()));
    let mut c2 = LoroCore::default();
    c2.subscribe_deep(Box::new(move |_event| {
        // println!("event: {:?}", _event);
    }));
    c2.decode(&input).unwrap();
    assert_eq(&c1, &c2);

    // snapshot
    println!("snapshot");
    let input = c1.encode_all();
    let mut c2 = LoroCore::default();
    c2.subscribe_deep(Box::new(move |_event| {
        // println!("event: {:?}", _event);
    }));
    c2.decode(&input).unwrap();
    assert_eq(&c1, &c2);
}

#[ctor]
fn init_color_backtrace() {
    color_backtrace::install();
}
