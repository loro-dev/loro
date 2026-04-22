use loro::{
    Container, ContainerTrait, ContainerType, LoroDoc, LoroList, LoroMap, LoroMovableList,
    LoroResult, LoroText, LoroTree, ToJson, TreeParentId,
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

#[cfg(feature = "counter")]
use loro::LoroCounter;

fn seed_container(container: &Container, label: &str) -> LoroResult<()> {
    match container {
        Container::Text(text) => text.insert(0, label)?,
        Container::Map(map) => map.insert("label", label)?,
        Container::List(list) => list.push(label)?,
        Container::MovableList(list) => list.push(label)?,
        Container::Tree(tree) => {
            let node = tree.create(TreeParentId::Root)?;
            tree.get_meta(node)?.insert("label", label)?;
        }
        #[cfg(feature = "counter")]
        Container::Counter(counter) => counter.increment(label.len() as f64)?,
        Container::Unknown(_) => unreachable!("Container::new cannot create Unknown"),
    }
    Ok(())
}

fn container_json(container: &Container) -> Value {
    match container {
        Container::Text(text) => json!(text.to_string()),
        Container::Map(map) => map.get_deep_value().to_json_value(),
        Container::List(list) => list.get_deep_value().to_json_value(),
        Container::MovableList(list) => list.get_deep_value().to_json_value(),
        Container::Tree(tree) => tree.get_value_with_meta().to_json_value(),
        #[cfg(feature = "counter")]
        Container::Counter(counter) => json!(counter.get()),
        Container::Unknown(_) => unreachable!("test never constructs unknown containers"),
    }
}

fn expected_json(kind: ContainerType, label: &str) -> Value {
    match kind {
        ContainerType::Text => json!(label),
        ContainerType::Map => json!({ "label": label }),
        ContainerType::List => json!([label]),
        ContainerType::MovableList => json!([label]),
        ContainerType::Tree => json!([{
            "id": Value::String(String::new()),
            "parent": Value::Null,
            "meta": { "label": label },
            "index": 0,
            "children": [],
            "fractional_index": "80",
        }]),
        #[cfg(feature = "counter")]
        ContainerType::Counter => json!(label.len() as f64),
        ContainerType::Unknown(_) => unreachable!("Container::new cannot create Unknown"),
    }
}

fn assert_tree_shape_matches(value: Value, label: &str) {
    let nodes = value.as_array().expect("tree value should be an array");
    assert_eq!(nodes.len(), 1);
    assert!(nodes[0]["id"].as_str().is_some());
    assert_eq!(nodes[0]["parent"], Value::Null);
    assert_eq!(nodes[0]["meta"], json!({ "label": label }));
    assert_eq!(nodes[0]["index"], 0);
    if let Some(children) = nodes[0].get("children") {
        assert_eq!(children, &json!([]));
    }
    if let Some(fractional_index) = nodes[0].get("fractional_index") {
        assert!(fractional_index.as_str().is_some());
    }
}

fn assert_container_value(container: &Container, label: &str) {
    let value = container_json(container);
    if container.get_type() == ContainerType::Tree {
        assert_tree_shape_matches(value, label);
    } else {
        assert_eq!(value, expected_json(container.get_type(), label));
    }
}

#[test]
fn container_enum_trait_dispatch_attaches_all_container_kinds() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(101)?;
    let root = doc.get_map("root");

    let kinds = vec![
        ContainerType::Text,
        ContainerType::Map,
        ContainerType::List,
        ContainerType::MovableList,
        ContainerType::Tree,
        #[cfg(feature = "counter")]
        ContainerType::Counter,
    ];

    let mut attached = Vec::new();
    let mut detached = Vec::new();
    for kind in kinds {
        let label = format!("{kind:?}").to_lowercase();
        let container = Container::new(kind);
        seed_container(&container, &label)?;

        assert!(!container.is_attached());
        assert!(container.doc().is_none());
        assert!(container.get_attached().is_none());
        assert_eq!(container.get_type(), kind);
        assert_container_value(&container, &label);

        let attached_container = root.insert_container(&label, container.clone())?;
        assert!(attached_container.is_attached());
        assert!(attached_container.doc().is_some());
        assert_eq!(attached_container.get_type(), kind);
        assert_eq!(attached_container.id().container_type(), kind);
        assert!(container.get_attached().is_some());
        assert_container_value(&attached_container, &label);

        detached.push((label, container));
        attached.push(attached_container);
    }

    doc.commit();
    let deep = doc.get_deep_value().to_json_value();
    assert_eq!(deep["root"]["text"], json!("text"));
    assert_eq!(deep["root"]["map"], json!({ "label": "map" }));
    assert_eq!(deep["root"]["list"], json!(["list"]));
    assert_eq!(deep["root"]["movablelist"], json!(["movablelist"]));
    assert_tree_shape_matches(deep["root"]["tree"].clone(), "tree");
    #[cfg(feature = "counter")]
    assert_eq!(deep["root"]["counter"], json!(7.0));

    for (label, detached_container) in &detached {
        let attached_container = detached_container
            .get_attached()
            .expect("detached enum container should remember its attached peer");
        assert!(attached_container.is_attached());
        assert_container_value(&attached_container, label);
    }

    for container in &attached {
        assert!(doc.has_container(&container.id()));
        assert!(container.doc().is_some());
        assert!(!container.is_deleted());
    }

    root.delete("text")?;
    let text_container = attached
        .iter()
        .find(|container| container.get_type() == ContainerType::Text)
        .unwrap();
    assert!(text_container.is_deleted());

    Ok(())
}

#[test]
fn container_enum_trait_dispatch_attaches_inside_lists_and_converts_back() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(202)?;
    let list = doc.get_list("containers");

    let text = Container::new(ContainerType::Text);
    seed_container(&text, "inside-list")?;
    let map = Container::new(ContainerType::Map);
    seed_container(&map, "inside-list-map")?;
    let tree = Container::new(ContainerType::Tree);
    seed_container(&tree, "inside-list-tree")?;

    let attached_text = list.push_container(text.clone())?;
    let attached_map = list.push_container(map.clone())?;
    let attached_tree = list.push_container(tree.clone())?;
    doc.commit();

    assert!(attached_text.is_attached());
    assert!(attached_map.is_attached());
    assert!(attached_tree.is_attached());
    assert!(text.get_attached().is_some());
    assert!(map.get_attached().is_some());
    assert!(tree.get_attached().is_some());

    let roundtrip = (0..list.len())
        .map(|index| list.get(index).unwrap().into_container().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        roundtrip
            .iter()
            .map(Container::get_type)
            .collect::<Vec<_>>(),
        vec![ContainerType::Text, ContainerType::Map, ContainerType::Tree]
    );

    assert!(LoroText::try_from_container(roundtrip[0].clone()).is_some());
    assert!(LoroMap::try_from_container(roundtrip[1].clone()).is_some());
    assert!(LoroTree::try_from_container(roundtrip[2].clone()).is_some());
    assert!(LoroList::try_from_container(roundtrip[0].clone()).is_none());
    assert!(LoroMovableList::try_from_container(roundtrip[1].clone()).is_none());
    #[cfg(feature = "counter")]
    assert!(LoroCounter::try_from_container(roundtrip[2].clone()).is_none());

    let deep = list.get_deep_value().to_json_value();
    assert_eq!(deep[0], json!("inside-list"));
    assert_eq!(deep[1], json!({ "label": "inside-list-map" }));
    assert_tree_shape_matches(deep[2].clone(), "inside-list-tree");

    Ok(())
}

#[test]
fn attached_containers_can_be_inserted_through_the_container_enum_as_independent_copies(
) -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(303)?;

    let source = doc.get_map("source");
    let text = source.insert_container("text", LoroText::new())?;
    text.insert(0, "copy text")?;

    let map = source.insert_container("map", LoroMap::new())?;
    map.insert("plain", 1)?;
    let map_child = map.insert_container("child", LoroText::new())?;
    map_child.insert(0, "child text")?;

    let list = source.insert_container("list", LoroList::new())?;
    list.push("head")?;
    let list_child = list.push_container(LoroMap::new())?;
    list_child.insert("kind", "nested")?;

    let movable = source.insert_container("movable", LoroMovableList::new())?;
    movable.push("left")?;
    let movable_child = movable.push_container(LoroText::new())?;
    movable_child.insert(0, "right")?;

    let tree = source.insert_container("tree", LoroTree::new())?;
    let root = tree.create(TreeParentId::Root)?;
    tree.get_meta(root)?.insert("label", "tree")?;

    #[cfg(feature = "counter")]
    let counter = {
        let counter = source.insert_container("counter", LoroCounter::new())?;
        counter.increment(4.0)?;
        counter
    };

    doc.commit();

    let copies = doc.get_map("copies");
    let text_copy = copies.insert_container("text", text.to_container())?;
    let map_copy = copies.insert_container("map", map.to_container())?;
    let list_copy = copies.insert_container("list", list.to_container())?;
    let movable_copy = copies.insert_container("movable", movable.to_container())?;
    let tree_copy = copies.insert_container("tree", tree.to_container())?;
    #[cfg(feature = "counter")]
    let counter_copy = copies.insert_container("counter", counter.to_container())?;

    text.insert(text.len_unicode(), " changed")?;
    map_child.insert(map_child.len_unicode(), " source")?;
    list.push("tail")?;
    movable.set(0, "changed")?;
    tree.get_meta(root)?.insert("label", "source tree")?;
    #[cfg(feature = "counter")]
    counter.increment(2.0)?;

    assert_eq!(container_json(&text_copy), json!("copy text"));
    assert_eq!(
        container_json(&map_copy),
        json!({ "plain": 1, "child": "child text" })
    );
    assert_eq!(
        container_json(&list_copy),
        json!(["head", { "kind": "nested" }])
    );
    assert_eq!(container_json(&movable_copy), json!(["left", "right"]));
    assert_tree_shape_matches(container_json(&tree_copy), "tree");
    #[cfg(feature = "counter")]
    assert_eq!(container_json(&counter_copy), json!(4.0));

    let copied_tree = tree_copy.into_tree().unwrap();
    let copied_root = copied_tree.roots()[0];
    copied_tree.get_meta(copied_root)?.insert("copied", true)?;
    assert!(tree.get_meta(root)?.get("copied").is_none());

    Ok(())
}
