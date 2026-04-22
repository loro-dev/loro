use std::sync::Arc;

use loro::{
    Container, ContainerTrait, LoroDoc, LoroList, LoroMap, LoroMovableList, LoroResult, LoroText,
    LoroTree, ToJson, TreeID, TreeParentId, ValueOrContainer,
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

fn expect_text(value: ValueOrContainer) -> LoroText {
    match value {
        ValueOrContainer::Container(Container::Text(text)) => text,
        other => panic!("expected text container, found {other:?}"),
    }
}

fn expect_list(value: ValueOrContainer) -> LoroList {
    match value {
        ValueOrContainer::Container(Container::List(list)) => list,
        other => panic!("expected list container, found {other:?}"),
    }
}

fn expect_movable_list(value: ValueOrContainer) -> LoroMovableList {
    match value {
        ValueOrContainer::Container(Container::MovableList(list)) => list,
        other => panic!("expected movable list container, found {other:?}"),
    }
}

fn expect_tree(value: ValueOrContainer) -> LoroTree {
    match value {
        ValueOrContainer::Container(Container::Tree(tree)) => tree,
        other => panic!("expected tree container, found {other:?}"),
    }
}

fn expect_root_array(value: &Value) -> &Vec<Value> {
    value.as_array().expect("tree json should be an array")
}

#[test]
fn detached_containers_keep_local_state_until_reattached_and_then_expose_live_clones(
) -> LoroResult<()> {
    let bundle = LoroMap::new();
    let bundle_probe = bundle.clone();

    assert!(!bundle.is_attached());
    assert!(bundle.doc().is_none());
    assert!(bundle.get_attached().is_none());
    assert!(bundle.subscribe(Arc::new(|_| {})).is_none());

    bundle.insert("title", "draft")?;
    bundle.insert("count", 1)?;

    let text = bundle.insert_container("text", LoroText::new())?;
    text.insert(0, "hello")?;

    let list = bundle.insert_container("list", LoroList::new())?;
    list.push("a")?;
    list.push("b")?;

    let movable = bundle.insert_container("ml", LoroMovableList::new())?;
    movable.insert(0, "x")?;
    movable.insert(1, "y")?;

    let tree = bundle.insert_container("tree", LoroTree::new())?;
    let root = tree.create(None)?;
    let child = tree.create(root)?;
    let root_meta = tree.get_meta(root)?;
    root_meta.insert("color", "red")?;
    let details = root_meta.insert_container("details", LoroMap::new())?;
    details.insert("kind", "leaf")?;
    let child_meta = tree.get_meta(child)?;
    child_meta.insert("name", "child")?;

    assert!(tree.contains(root));
    assert!(tree.contains(child));
    assert_eq!(tree.parent(root), Some(TreeParentId::Root));
    assert_eq!(tree.parent(child), Some(TreeParentId::Node(root)));
    assert_eq!(tree.children_num(TreeParentId::Root), Some(1));
    assert_eq!(tree.children(root), Some(vec![child]));
    assert_eq!(tree.nodes().len(), 2);

    assert!(!text.is_attached());
    assert!(text.doc().is_none());
    assert!(text.get_attached().is_none());
    assert!(text.subscribe(Arc::new(|_| {})).is_none());

    assert!(!list.is_attached());
    assert!(list.doc().is_none());
    assert!(list.get_attached().is_none());

    assert!(!movable.is_attached());
    assert!(movable.doc().is_none());
    assert!(movable.get_attached().is_none());

    assert!(!tree.is_attached());
    assert!(tree.doc().is_none());
    assert!(tree.get_attached().is_none());
    assert!(root_meta.doc().is_none());
    assert!(details.doc().is_none());
    assert!(child_meta.doc().is_none());

    assert_eq!(text.to_string(), "hello");
    assert_eq!(list.get_deep_value().to_json_value(), json!(["a", "b"]));
    assert_eq!(movable.get_deep_value().to_json_value(), json!(["x", "y"]));
    let detached_tree_json = tree.get_value_with_meta().to_json_value();
    let detached_tree_nodes = expect_root_array(&detached_tree_json);
    assert_eq!(detached_tree_nodes.len(), 2);
    assert!(detached_tree_nodes
        .iter()
        .any(|node| node["parent"].is_null() && node["meta"]["color"] == "red"));
    assert!(detached_tree_nodes
        .iter()
        .any(|node| { node["parent"].is_null() && node["meta"]["details"]["kind"] == "leaf" }));
    assert!(detached_tree_nodes.iter().any(|node| {
        node["parent"] == json!(root.to_string()) && node["meta"]["name"] == "child"
    }));

    let doc = LoroDoc::new();
    doc.set_peer_id(7)?;
    let root_map = doc.get_map("root");
    let attached_bundle = root_map.insert_container("bundle", bundle)?;

    assert!(attached_bundle.is_attached());
    assert!(attached_bundle.doc().is_some());
    assert!(attached_bundle.get_attached().is_some());
    assert!(attached_bundle.subscribe(Arc::new(|_| {})).is_some());
    assert!(bundle_probe.get_attached().is_some());
    assert!(!bundle_probe.is_attached());
    assert!(bundle_probe.doc().is_none());

    let detached_text = expect_text(bundle_probe.get("text").unwrap());
    assert!(!detached_text.is_attached());
    assert!(detached_text.doc().is_none());
    assert!(detached_text.get_attached().is_some());
    assert_eq!(detached_text.to_string(), "hello");

    let detached_list = expect_list(bundle_probe.get("list").unwrap());
    assert!(!detached_list.is_attached());
    assert!(detached_list.doc().is_none());
    assert!(detached_list.get_attached().is_some());
    assert_eq!(
        detached_list.get_deep_value().to_json_value(),
        json!(["a", "b"])
    );

    let detached_movable = expect_movable_list(bundle_probe.get("ml").unwrap());
    assert!(!detached_movable.is_attached());
    assert!(detached_movable.doc().is_none());
    assert!(detached_movable.get_attached().is_some());
    assert_eq!(
        detached_movable.get_deep_value().to_json_value(),
        json!(["x", "y"])
    );

    let detached_tree = expect_tree(bundle_probe.get("tree").unwrap());
    assert!(!detached_tree.is_attached());
    assert!(detached_tree.doc().is_none());
    assert!(detached_tree.get_attached().is_some());
    assert_eq!(
        detached_tree.get_value().to_json_value(),
        tree.get_value().to_json_value()
    );

    assert!(text.get_attached().is_some());
    assert!(!text.is_attached());
    assert!(text.doc().is_none());
    assert!(list.get_attached().is_some());
    assert!(!list.is_attached());
    assert!(list.doc().is_none());
    assert!(movable.get_attached().is_some());
    assert!(!movable.is_attached());
    assert!(movable.doc().is_none());
    assert!(tree.get_attached().is_some());
    assert!(!tree.is_attached());
    assert!(tree.doc().is_none());
    assert!(root_meta.get_attached().is_some());
    assert!(details.get_attached().is_some());
    assert!(child_meta.get_attached().is_some());

    let attached_bundle_from_probe = bundle_probe.get_attached().unwrap();
    assert!(attached_bundle_from_probe.is_attached());
    assert!(attached_bundle_from_probe.doc().is_some());

    let attached_text = expect_text(attached_bundle.get("text").unwrap())
        .get_attached()
        .unwrap();
    assert!(attached_text.is_attached());
    assert!(attached_text.doc().is_some());
    assert_eq!(attached_text.to_string(), "hello");
    assert_eq!(attached_text.doc().unwrap().peer_id(), 7);
    assert_eq!(attached_text.to_string(), "hello");

    let fetched_text = doc.get_container(attached_text.id()).unwrap();
    assert!(LoroText::try_from_container(fetched_text.clone()).is_some());
    assert!(LoroList::try_from_container(fetched_text).is_none());

    let attached_list = expect_list(attached_bundle.get("list").unwrap())
        .get_attached()
        .unwrap();
    assert!(attached_list.is_attached());
    assert!(attached_list.doc().is_some());
    assert_eq!(
        attached_list.get_deep_value().to_json_value(),
        json!(["a", "b"])
    );

    let attached_movable = expect_movable_list(attached_bundle.get("ml").unwrap())
        .get_attached()
        .unwrap();
    assert!(attached_movable.is_attached());
    assert!(attached_movable.doc().is_some());
    assert_eq!(
        attached_movable.get_deep_value().to_json_value(),
        json!(["x", "y"])
    );

    let attached_tree = expect_tree(attached_bundle.get("tree").unwrap())
        .get_attached()
        .unwrap();
    assert!(attached_tree.is_attached());
    assert!(attached_tree.doc().is_some());
    assert_eq!(attached_tree.get_nodes(false).len(), 2);
    assert_eq!(attached_tree.get_nodes(true).len(), 2);
    let attached_roots = attached_tree.roots();
    assert_eq!(attached_roots.len(), 1);
    let attached_root = attached_roots[0];
    assert_eq!(
        attached_tree.parent(attached_root),
        Some(TreeParentId::Root)
    );
    let attached_children = attached_tree.children(attached_root).unwrap();
    assert_eq!(attached_children.len(), 1);
    let attached_child = attached_children[0];
    assert_eq!(
        attached_tree.parent(attached_child),
        Some(TreeParentId::Node(attached_root))
    );
    let attached_root_meta = attached_tree.get_meta(attached_root)?;
    let attached_details = attached_root_meta
        .get("details")
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap();
    assert!(attached_root_meta.doc().is_some());
    assert!(attached_details.doc().is_some());

    attached_text.insert(5, "!")?;
    attached_list.push("tail")?;
    attached_movable.mov(0, 1)?;
    attached_root_meta.insert("theme", "dark")?;
    attached_details.insert("depth", 2)?;
    let sibling = attached_tree.create(TreeParentId::Root)?;
    attached_tree.get_meta(sibling)?.insert("color", "blue")?;

    let tree_json_with_sibling = attached_tree.get_value_with_meta().to_json_value();
    let tree_nodes = expect_root_array(&tree_json_with_sibling);
    assert_eq!(tree_nodes.len(), 2);
    assert_eq!(tree_nodes[0]["meta"]["color"], "red");
    assert_eq!(tree_nodes[0]["meta"]["theme"], "dark");
    assert_eq!(tree_nodes[0]["meta"]["details"]["kind"], "leaf");
    assert_eq!(tree_nodes[0]["meta"]["details"]["depth"], 2);
    assert_eq!(tree_nodes[0]["children"][0]["meta"]["name"], "child");
    assert_eq!(tree_nodes[1]["meta"]["color"], "blue");

    let shallow_tree_json = attached_tree.get_value().to_json_value();
    assert_ne!(shallow_tree_json, tree_json_with_sibling);

    attached_tree.delete(sibling)?;
    assert!(attached_tree.contains(sibling));
    assert!(attached_tree.is_node_deleted(&sibling)?);
    assert_eq!(attached_tree.parent(sibling), Some(TreeParentId::Deleted));
    assert_eq!(attached_tree.children(sibling), None);

    attached_text.delete(0, attached_text.len_unicode())?;
    attached_list.clear()?;
    attached_movable.clear()?;
    attached_details.clear()?;
    attached_root_meta.clear()?;

    assert!(attached_text.is_empty());
    assert!(attached_list.is_empty());
    assert!(attached_movable.is_empty());
    assert!(attached_root_meta.is_empty());
    assert!(attached_details.is_empty());
    let final_tree_json = attached_tree.get_value_with_meta().to_json_value();
    let final_tree_nodes = expect_root_array(&final_tree_json);
    assert_eq!(final_tree_nodes.len(), 1);
    assert_eq!(final_tree_nodes[0]["meta"], json!({}));
    assert_eq!(final_tree_nodes[0]["children"].as_array().unwrap().len(), 1);
    assert_eq!(final_tree_nodes[0]["children"][0]["meta"]["name"], "child");
    assert_eq!(
        final_tree_nodes[0]["children"][0]["children"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    doc.commit();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "root": {
                "bundle": {
                    "title": "draft",
                    "count": 1,
                    "text": "",
                    "list": [],
                    "ml": [],
                    "tree": final_tree_json,
                }
            }
        })
    );

    Ok(())
}

#[test]
fn detached_container_error_branches_follow_the_contract() -> LoroResult<()> {
    let map = LoroMap::new();
    let list = LoroList::new();
    let movable = LoroMovableList::new();
    let text = LoroText::new();
    let tree = LoroTree::new();

    assert!(!map.is_attached());
    assert!(map.doc().is_none());
    assert!(map.get_attached().is_none());
    assert!(map.subscribe(Arc::new(|_| {})).is_none());

    assert!(!list.is_attached());
    assert!(list.doc().is_none());
    assert!(list.get_attached().is_none());

    assert!(!movable.is_attached());
    assert!(movable.doc().is_none());
    assert!(movable.get_attached().is_none());

    assert!(!text.is_attached());
    assert!(text.doc().is_none());
    assert!(text.get_attached().is_none());

    assert!(!tree.is_attached());
    assert!(tree.doc().is_none());
    assert!(tree.get_attached().is_none());

    assert_eq!(list.pop()?, None);
    assert!(movable.pop()?.is_none());

    let missing = TreeID::new(99_999, 88_888);
    assert!(tree.get_meta(missing).is_err());
    assert_eq!(tree.children(missing), None);
    assert_eq!(tree.parent(missing), None);
    assert!(tree.is_node_deleted(&missing).is_err());
    assert!(!tree.contains(missing));

    Ok(())
}
