use loro::{
    ContainerTrait, ExportMode, LoroDoc, LoroMap, LoroResult, LoroTree, ToJson, TreeID,
    TreeParentId,
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

fn assert_tree_error<T: core::fmt::Debug>(result: LoroResult<T>, expected: &str) {
    let err = result.expect_err("tree operation should fail");
    assert_eq!(err.to_string(), expected);
}

#[test]
fn detached_tree_attaches_into_map_and_keeps_contracts() -> LoroResult<()> {
    let tree = LoroTree::new();
    let probe = tree.clone();

    assert!(!tree.is_attached());
    assert!(tree.doc().is_none());
    assert!(tree.get_attached().is_none());
    assert!(tree.is_empty());
    assert_eq!(tree.children(TreeParentId::Root), None);
    assert_eq!(tree.children_num(TreeParentId::Root), None);

    let root = tree.create(TreeParentId::Root)?;
    let child = tree.create(root)?;
    let root_meta = tree.get_meta(root)?;
    root_meta.insert("title", "root")?;
    root_meta
        .insert_container("details", LoroMap::new())?
        .insert("kind", "leaf")?;
    tree.get_meta(child)?.insert("title", "child")?;

    assert!(tree.contains(root));
    assert!(tree.contains(child));
    assert_eq!(tree.parent(root), Some(TreeParentId::Root));
    assert_eq!(tree.parent(child), Some(TreeParentId::Node(root)));
    assert_eq!(tree.children(TreeParentId::Root), Some(vec![root]));
    assert_eq!(tree.children(root), Some(vec![child]));
    assert_eq!(tree.children_num(root), Some(1));
    assert_eq!(tree.get_last_move_id(&root), None);

    let doc = LoroDoc::new();
    doc.set_peer_id(7)?;
    let workspace = doc.get_map("workspace");
    let attached_tree = workspace.insert_container("outline", tree)?;

    assert!(attached_tree.is_attached());
    assert!(attached_tree.doc().is_some());
    assert!(attached_tree.get_attached().is_some());
    assert!(probe.get_attached().is_some());
    assert!(!probe.is_attached());
    assert!(probe.doc().is_none());
    assert_eq!(attached_tree.roots().len(), 1);
    let attached_root = attached_tree.roots()[0];
    assert_eq!(
        attached_tree.children(TreeParentId::Root),
        Some(vec![attached_root])
    );
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
    assert!(attached_tree.get_meta(attached_root)?.doc().is_some());
    assert!(attached_tree.get_meta(attached_child)?.doc().is_some());

    let attached_root_meta = attached_tree.get_meta(attached_root)?;
    attached_root_meta.insert("status", "live")?;
    assert_eq!(
        attached_tree.get_value_with_meta().to_json_value(),
        json!([
            {
                "id": attached_root.to_string(),
                "parent": null,
                "meta": {
                    "details": {"kind": "leaf"},
                    "status": "live",
                    "title": "root"
                },
                "fractional_index": "80",
                "index": 0,
                "children": [
                    {
                        "id": attached_child.to_string(),
                        "parent": attached_root.to_string(),
                        "meta": {"title": "child"},
                        "fractional_index": "80",
                        "index": 0,
                        "children": []
                    }
                ]
            }
        ])
    );

    doc.commit();
    let snapshot = doc.export(ExportMode::Snapshot)?;
    let restored = LoroDoc::from_snapshot(&snapshot)?;
    assert_eq!(deep_json(&restored), deep_json(&doc));
    let restored_tree = restored.get_map("workspace");
    let restored_tree = restored_tree
        .get("outline")
        .unwrap()
        .into_container()
        .unwrap()
        .into_tree()
        .unwrap();
    assert_eq!(restored_tree.roots().len(), 1);
    let restored_root = restored_tree.roots()[0];
    assert_eq!(
        restored_tree.children(TreeParentId::Root),
        Some(vec![restored_root])
    );
    let restored_children = restored_tree.children(restored_root).unwrap();
    assert_eq!(restored_children.len(), 1);
    assert_eq!(
        restored_tree.parent(restored_children[0]),
        Some(TreeParentId::Node(restored_root))
    );

    Ok(())
}

#[test]
fn attached_tree_fractional_index_toggle_and_cycle_contracts_follow_docs() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(11)?;
    let tree = doc.get_tree("outline");

    assert!(tree.is_fractional_index_enabled());
    tree.enable_fractional_index(1);
    assert!(tree.is_fractional_index_enabled());

    assert_tree_error(
        tree.create(TreeParentId::Deleted),
        "Movable Tree Error: The provided parent id is invalid",
    );
    assert_tree_error(
        tree.create(TreeParentId::Unexist),
        "Movable Tree Error: The provided parent id is invalid",
    );

    let root = tree.create(TreeParentId::Root)?;
    let mut children = Vec::new();
    for index in 0..18 {
        children.push(tree.create_at(root, index)?);
    }
    assert_eq!(tree.children(root).unwrap(), children);
    assert_eq!(tree.children_num(root), Some(18));
    assert!(tree.fractional_index(root).is_some());

    assert_tree_error(
        tree.create_at(root, 19),
        "Movable Tree Error: The index(19) should be <= the length of children (18)",
    );

    let same_index_child = children[3];
    tree.mov_to(same_index_child, root, 3)?;
    assert_eq!(tree.children(root).unwrap(), children);

    assert_tree_error(
        tree.mov(root, same_index_child),
        "Movable Tree Error: `Cycle move` occurs when moving tree nodes.",
    );

    tree.disable_fractional_index();
    assert!(!tree.is_fractional_index_enabled());
    assert_tree_error(
        tree.create_at(root, 0),
        "Movable Tree Error: Fractional index is not enabled, you should enable it first by `LoroTree::set_enable_fractional_index`",
    );
    assert_tree_error(
        tree.mov_to(children[0], root, 0),
        "Movable Tree Error: Fractional index is not enabled, you should enable it first by `LoroTree::set_enable_fractional_index`",
    );
    assert_tree_error(
        tree.mov_before(children[1], children[0]),
        "Movable Tree Error: Fractional index is not enabled, you should enable it first by `LoroTree::set_enable_fractional_index`",
    );
    assert_tree_error(
        tree.mov_after(children[1], children[0]),
        "Movable Tree Error: Fractional index is not enabled, you should enable it first by `LoroTree::set_enable_fractional_index`",
    );

    let attached_to_root = tree.create(root)?;
    tree.mov(attached_to_root, TreeParentId::Root)?;
    assert_eq!(tree.parent(attached_to_root), Some(TreeParentId::Root));
    assert_eq!(
        tree.children(TreeParentId::Root),
        Some(vec![attached_to_root, root])
    );

    tree.enable_fractional_index(1);
    assert!(tree.is_fractional_index_enabled());
    tree.mov_to(attached_to_root, TreeParentId::Root, 0)?;
    assert_eq!(
        tree.children(TreeParentId::Root),
        Some(vec![attached_to_root, root])
    );

    Ok(())
}

#[test]
fn attached_tree_delete_checkout_and_restore_keep_tree_history_contracts() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(19)?;
    let tree = doc.get_tree("outline");
    tree.enable_fractional_index(1);

    let mut roots = Vec::new();
    for index in 0..18 {
        let root = tree.create(TreeParentId::Root)?;
        roots.push(root);
        tree.get_meta(root)?
            .insert("name", format!("root-{index}"))?;
        if index % 2 == 0 {
            tree.get_meta(root)?
                .insert_container("meta", LoroMap::new())?
                .insert("even", true)?;
        }
    }
    doc.commit();
    let before_delete_frontiers = doc.oplog_frontiers();
    let before_delete_snapshot = tree.get_value_with_meta().to_json_value();
    let before_delete_nodes = tree.get_nodes(false);
    assert_eq!(before_delete_nodes.len(), 18);
    assert_eq!(tree.get_nodes(true).len(), 18);
    assert_eq!(tree.children(TreeParentId::Root), Some(roots.clone()));

    for root in roots.iter().copied() {
        tree.delete(root)?;
    }
    assert!(tree.is_empty());
    assert_eq!(tree.roots(), Vec::<TreeID>::new());
    assert_eq!(tree.children(TreeParentId::Root), Some(vec![]));
    assert_eq!(tree.children_num(TreeParentId::Root), Some(0));
    assert!(tree.get_nodes(false).is_empty());
    assert_eq!(tree.get_nodes(true).len(), 18);
    assert!(tree.is_node_deleted(&roots[0])?);
    assert_eq!(tree.parent(roots[0]), Some(TreeParentId::Deleted));

    doc.commit();
    let after_delete_snapshot = doc.export(ExportMode::Snapshot)?;
    let after_delete_doc = LoroDoc::from_snapshot(&after_delete_snapshot)?;
    assert!(after_delete_doc.get_tree("outline").is_empty());

    doc.checkout(&before_delete_frontiers)?;
    assert_eq!(
        tree.get_value_with_meta().to_json_value(),
        before_delete_snapshot
    );
    assert_eq!(tree.children(TreeParentId::Root), Some(roots.clone()));
    assert_eq!(tree.get_nodes(false).len(), 18);
    assert_eq!(tree.get_nodes(true).len(), 18);
    assert!(tree.contains(roots[0]));
    assert!(!tree.is_empty());

    doc.checkout_to_latest();
    assert!(tree.is_empty());
    assert_eq!(tree.get_nodes(false).len(), 0);
    assert_eq!(tree.children(TreeParentId::Root), Some(vec![]));
    assert_eq!(tree.get_nodes(true).len(), 18);
    assert_eq!(deep_json(&doc), deep_json(&after_delete_doc));

    Ok(())
}
