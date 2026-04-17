use loro_internal::handler::HandlerTrait;
use loro_internal::loro::ExportMode;
use loro_internal::LoroDoc;
use loro_internal::ToJson;

#[test]
fn test_mergeable_list() {
    let doc = LoroDoc::new();
    let map = doc.get_map("map");
    let list = map.get_mergeable_list("list").unwrap();
    list.insert(0, 1).unwrap();

    let list2 = map.get_mergeable_list("list").unwrap();
    assert_eq!(list.id(), list2.id());
    assert_eq!(
        list.get_value().to_json_value(),
        list2.get_value().to_json_value()
    );
}

#[test]
fn test_concurrent_mergeable_list() {
    let doc1 = LoroDoc::new();
    doc1.set_peer_id(1).unwrap();
    let map1 = doc1.get_map("map");
    let list1 = map1.get_mergeable_list("list").unwrap();
    list1.insert(0, 1).unwrap();

    let doc2 = LoroDoc::new();
    doc2.set_peer_id(2).unwrap();
    let map2 = doc2.get_map("map");
    let list2 = map2.get_mergeable_list("list").unwrap();
    list2.insert(0, 2).unwrap();

    doc1.import(&doc2.export(ExportMode::snapshot()).unwrap())
        .unwrap();
    doc2.import(&doc1.export(ExportMode::snapshot()).unwrap())
        .unwrap();

    let list1 = map1.get_mergeable_list("list").unwrap();
    let list2 = map2.get_mergeable_list("list").unwrap();

    assert_eq!(list1.id(), list2.id());
    assert_eq!(list1.len(), 2);
    assert_eq!(list2.len(), 2);

    // Check that it is indeed a mergeable container (Root ID)
    assert!(list1.id().is_mergeable());
}

#[test]
fn test_serialization_hides_mergeable() {
    let doc = LoroDoc::new();
    let map = doc.get_map("map");
    let _list = map.get_mergeable_list("list").unwrap();

    let json = doc.get_value();
    // The mergeable container should NOT appear at the root level
    // It should only appear under "map"

    // doc.get_value() returns LoroValue::Map
    let map_val = json.as_map().unwrap();
    // It should contain "map"
    assert!(map_val.contains_key("map"));

    // It should NOT contain the mergeable list ID as a key (because it's a Root container)
    // Root containers usually appear in `doc.get_value()` if they are top-level.

    let root_keys: Vec<String> = map_val.keys().cloned().collect();
    assert!(root_keys.contains(&"map".to_string()));

    // The mergeable list has a name like "cid:root-map:Map/list".
    // We want to ensure this name is NOT in root_keys.
    for key in root_keys {
        assert!(!key.contains('/'));
    }
}

#[test]
fn test_mergeable_map() {
    let doc = LoroDoc::new();
    let map = doc.get_map("map");
    let sub_map = map.get_mergeable_map("sub_map").unwrap();
    sub_map.insert("key", "value").unwrap();

    let sub_map2 = map.get_mergeable_map("sub_map").unwrap();
    assert_eq!(sub_map.id(), sub_map2.id());
    assert_eq!(
        sub_map.get_value().to_json_value(),
        sub_map2.get_value().to_json_value()
    );
}

#[test]
fn test_mergeable_text() {
    let doc = LoroDoc::new();
    let map = doc.get_map("map");
    let text = map.get_mergeable_text("text").unwrap();
    text.insert_utf8(0, "Hello").unwrap();

    let text2 = map.get_mergeable_text("text").unwrap();
    assert_eq!(text.id(), text2.id());
    assert_eq!(text.to_string(), text2.to_string());
}

#[test]
fn test_mergeable_tree() {
    let doc = LoroDoc::new();
    let map = doc.get_map("map");
    let tree = map.get_mergeable_tree("tree").unwrap();
    let root = tree.create(loro_internal::TreeParentId::Root).unwrap();

    let tree2 = map.get_mergeable_tree("tree").unwrap();
    assert_eq!(tree.id(), tree2.id());
    assert!(tree2.contains(root));
}

#[test]
fn test_mergeable_movable_list() {
    let doc = LoroDoc::new();
    let map = doc.get_map("map");
    let list = map.get_mergeable_movable_list("list").unwrap();
    list.insert(0, 1).unwrap();

    let list2 = map.get_mergeable_movable_list("list").unwrap();
    assert_eq!(list.id(), list2.id());
    assert_eq!(
        list.get_value().to_json_value(),
        list2.get_value().to_json_value()
    );
}

#[test]
fn test_nested_mergeable_containers() {
    let doc = LoroDoc::new();
    let map = doc.get_map("map");

    // Can we have a mergeable container inside a mergeable map?
    let mergeable_map = map.get_mergeable_map("mergeable_map").unwrap();
    let nested_mergeable_list = mergeable_map.get_mergeable_list("nested_list").unwrap();
    nested_mergeable_list.insert(0, "nested").unwrap();

    let mergeable_map2 = map.get_mergeable_map("mergeable_map").unwrap();
    let nested_mergeable_list2 = mergeable_map2.get_mergeable_list("nested_list").unwrap();

    assert_eq!(nested_mergeable_list.id(), nested_mergeable_list2.id());
    assert_eq!(
        nested_mergeable_list.get_value().to_json_value(),
        nested_mergeable_list2.get_value().to_json_value()
    );
}

#[test]
fn test_deep_nested_concurrent_merge() {
    let doc1 = LoroDoc::new();
    doc1.set_peer_id(1).unwrap();
    let doc2 = LoroDoc::new();
    doc2.set_peer_id(2).unwrap();

    // Peer 1 creates: Map("root") -> MergeableMap("level1") -> MergeableMap("level2") -> MergeableList("list") -> insert "A"
    {
        let root = doc1.get_map("root");
        let level1 = root.get_mergeable_map("level1").unwrap();
        let level2 = level1.get_mergeable_map("level2").unwrap();
        let list = level2.get_mergeable_list("list").unwrap();
        list.insert(0, "A").unwrap();
    }

    // Peer 2 creates: Map("root") -> MergeableMap("level1") -> MergeableMap("level2") -> MergeableList("list") -> insert "B"
    {
        let root = doc2.get_map("root");
        let level1 = root.get_mergeable_map("level1").unwrap();
        let level2 = level1.get_mergeable_map("level2").unwrap();
        let list = level2.get_mergeable_list("list").unwrap();
        list.insert(0, "B").unwrap();
    }

    // Merge
    doc1.import(&doc2.export(ExportMode::snapshot()).unwrap())
        .unwrap();
    doc2.import(&doc1.export(ExportMode::snapshot()).unwrap())
        .unwrap();

    // Verify
    let root = doc1.get_map("root");
    let level1 = root.get_mergeable_map("level1").unwrap();
    let level2 = level1.get_mergeable_map("level2").unwrap();
    let list = level2.get_mergeable_list("list").unwrap();

    assert_eq!(list.len(), 2);
    // Order depends on Lamport/PeerID, but both should be there.
    let val = list.get_value().to_json_value();
    let arr = val.as_array().unwrap();
    let items: Vec<&str> = arr.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(items.contains(&"A"));
    assert!(items.contains(&"B"));
}

#[test]
fn test_mixed_nested_concurrent_merge() {
    let doc1 = LoroDoc::new();
    doc1.set_peer_id(1).unwrap();
    let doc2 = LoroDoc::new();
    doc2.set_peer_id(2).unwrap();

    // Path: root -> map -> movable_list -> insert text
    // Peer 1
    {
        let root = doc1.get_map("root");
        let map = root.get_mergeable_map("map").unwrap();
        let list = map.get_mergeable_movable_list("list").unwrap();
        list.insert(0, "A").unwrap();
    }

    // Peer 2
    {
        let root = doc2.get_map("root");
        let map = root.get_mergeable_map("map").unwrap();
        let list = map.get_mergeable_movable_list("list").unwrap();
        list.insert(0, "B").unwrap();
    }

    // Merge
    doc1.import(&doc2.export(ExportMode::snapshot()).unwrap())
        .unwrap();
    doc2.import(&doc1.export(ExportMode::snapshot()).unwrap())
        .unwrap();

    // Verify
    let root = doc1.get_map("root");
    let map = root.get_mergeable_map("map").unwrap();
    let list = map.get_mergeable_movable_list("list").unwrap();

    assert_eq!(list.len(), 2);
}

#[test]
#[cfg(feature = "counter")]
fn test_mergeable_counter() {
    let doc = LoroDoc::new();
    let map = doc.get_map("map");
    let counter = map.get_mergeable_counter("counter").unwrap();
    counter.increment(1.0).unwrap();

    let counter2 = map.get_mergeable_counter("counter").unwrap();
    assert_eq!(counter.id(), counter2.id());
    assert_eq!(
        counter.get_value().to_json_value(),
        counter2.get_value().to_json_value()
    );
    assert_eq!(counter.get_value().to_json_value(), serde_json::json!(1.0));
}

#[test]
#[cfg(feature = "counter")]
fn test_nested_mergeable_counter_concurrent() {
    let doc1 = LoroDoc::new();
    doc1.set_peer_id(1).unwrap();
    let doc2 = LoroDoc::new();
    doc2.set_peer_id(2).unwrap();

    // Map -> MergeableMap -> MergeableCounter
    {
        let root = doc1.get_map("root");
        let map = root.get_mergeable_map("nested").unwrap();
        let counter = map.get_mergeable_counter("counter").unwrap();
        counter.increment(10.0).unwrap();
    }

    {
        let root = doc2.get_map("root");
        let map = root.get_mergeable_map("nested").unwrap();
        let counter = map.get_mergeable_counter("counter").unwrap();
        counter.increment(20.0).unwrap();
    }

    doc1.import(&doc2.export(ExportMode::snapshot()).unwrap())
        .unwrap();
    doc2.import(&doc1.export(ExportMode::snapshot()).unwrap())
        .unwrap();

    let root = doc1.get_map("root");
    let map = root.get_mergeable_map("nested").unwrap();
    let counter = map.get_mergeable_counter("counter").unwrap();

    assert_eq!(counter.get_value().to_json_value(), serde_json::json!(30.0));
}

#[test]
fn test_mergeable_container_path() {
    let doc = LoroDoc::new();
    let map = doc.get_map("map");
    let list = map.get_mergeable_list("list").unwrap();

    let path = doc.get_path_to_container(&list.id()).unwrap();
    // Path should be [map, list]
    assert_eq!(path.len(), 2);
    assert_eq!(path[0].1, loro_internal::event::Index::Key("map".into()));
    assert_eq!(path[1].1, loro_internal::event::Index::Key("list".into()));
}
