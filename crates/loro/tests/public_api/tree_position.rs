use loro::{
    ContainerTrait, ExportMode, LoroDoc, LoroMap, LoroResult, LoroTree, ToJson, TreeID, TreeNode,
    TreeParentId,
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

fn summarize_nodes(nodes: &[TreeNode]) -> Value {
    Value::Array(
        nodes
            .iter()
            .map(|node| {
                json!({
                    "id": node.id.to_string(),
                    "parent": format!("{:?}", node.parent),
                    "index": node.index,
                    "fractional_index": node.fractional_index.to_string(),
                })
            })
            .collect(),
    )
}

fn assert_tree_error<T: core::fmt::Debug>(result: LoroResult<T>, expected: &str) {
    let err = result.expect_err("tree operation should fail");
    assert_eq!(err.to_string(), expected);
}

#[test]
fn attached_tree_create_at_move_to_move_and_snapshot_import_keep_positions_and_meta(
) -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;

    let tree = doc.get_tree("outline");
    assert!(tree.is_empty());
    assert_eq!(tree.children(TreeParentId::Root), None);
    assert_eq!(tree.children_num(TreeParentId::Root), None);

    assert_tree_error(
        tree.create(TreeParentId::Deleted),
        "Movable Tree Error: The provided parent id is invalid",
    );
    assert_tree_error(
        tree.create(TreeParentId::Unexist),
        "Movable Tree Error: The provided parent id is invalid",
    );

    tree.enable_fractional_index(0);
    assert!(tree.is_fractional_index_enabled());

    let root = tree.create(TreeParentId::Root)?;
    let child_a = tree.create_at(root, 0)?;
    let child_b = tree.create_at(root, 1)?;
    let grandchild = tree.create_at(child_a, 0)?;

    assert_eq!(tree.fractional_index(root).as_deref(), Some("80"));
    assert_eq!(tree.fractional_index(child_a).as_deref(), Some("80"));
    assert_eq!(tree.fractional_index(grandchild).as_deref(), Some("80"));
    assert_eq!(tree.children(root), Some(vec![child_a, child_b]));
    assert_eq!(tree.children_num(root), Some(2));
    assert_eq!(tree.children(child_a), Some(vec![grandchild]));
    assert_eq!(tree.children_num(child_a), Some(1));

    let root_meta = tree.get_meta(root)?;
    root_meta.insert("title", "root")?;
    let root_details = root_meta.insert_container("details", LoroMap::new())?;
    root_details.insert("owner", "alice")?;
    tree.get_meta(child_a)?.insert("title", "child-a")?;
    tree.get_meta(child_b)?.insert("title", "child-b")?;
    tree.get_meta(grandchild)?.insert("title", "grandchild")?;

    doc.commit();

    tree.disable_fractional_index();
    assert!(!tree.is_fractional_index_enabled());
    assert_tree_error(
        tree.create_at(root, 0),
        "Movable Tree Error: Fractional index is not enabled, you should enable it first by `LoroTree::set_enable_fractional_index`",
    );
    assert_tree_error(
        tree.mov_to(child_b, root, 0),
        "Movable Tree Error: Fractional index is not enabled, you should enable it first by `LoroTree::set_enable_fractional_index`",
    );

    tree.mov(grandchild, TreeParentId::Root)?;
    assert_eq!(tree.parent(grandchild), Some(TreeParentId::Root));
    assert_eq!(tree.children(child_a), Some(vec![]));
    assert_eq!(tree.children_num(child_a), Some(0));

    tree.enable_fractional_index(0);
    assert!(tree.is_fractional_index_enabled());

    tree.mov_to(child_b, root, 0)?;
    tree.mov_to(grandchild, child_a, 0)?;

    assert_eq!(tree.children(root), Some(vec![child_b, child_a]));
    assert_eq!(tree.children_num(root), Some(2));
    assert_eq!(tree.children(child_a), Some(vec![grandchild]));
    assert_eq!(tree.children_num(child_a), Some(1));

    let before_delete_nodes = tree.get_nodes(false);
    let before_delete_all_nodes = tree.get_nodes(true);
    assert_eq!(
        before_delete_nodes
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![root, child_b, child_a, grandchild]
    );
    assert_eq!(
        before_delete_all_nodes
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![root, child_b, child_a, grandchild]
    );
    assert_eq!(before_delete_nodes[0].parent, TreeParentId::Root);
    assert_eq!(before_delete_nodes[1].parent, TreeParentId::Node(root));
    assert_eq!(before_delete_nodes[2].parent, TreeParentId::Node(root));
    assert_eq!(before_delete_nodes[3].parent, TreeParentId::Node(child_a));
    assert_eq!(before_delete_nodes[0].index, 0);
    assert_eq!(before_delete_nodes[1].index, 0);
    assert_eq!(before_delete_nodes[2].index, 1);
    assert_eq!(before_delete_nodes[3].index, 0);

    let before_delete_value = tree.get_value_with_meta().to_json_value();
    let before_delete_summary = summarize_nodes(&before_delete_nodes);

    let snapshot = doc.export(ExportMode::Snapshot)?;
    let restored = LoroDoc::from_snapshot(&snapshot)?;
    let restored_tree = restored.get_tree("outline");
    assert_eq!(deep_json(&restored), deep_json(&doc));
    assert_eq!(
        restored_tree.get_value_with_meta().to_json_value(),
        before_delete_value
    );
    assert_eq!(
        summarize_nodes(&restored_tree.get_nodes(false)),
        before_delete_summary
    );
    assert_eq!(
        summarize_nodes(&restored_tree.get_nodes(true)),
        summarize_nodes(&before_delete_all_nodes)
    );
    assert_eq!(restored_tree.children(root), Some(vec![child_b, child_a]));
    assert_eq!(restored_tree.children(child_a), Some(vec![grandchild]));
    assert_eq!(restored_tree.children_num(root), Some(2));
    assert_eq!(restored_tree.children_num(child_a), Some(1));

    let missing = TreeID::new(doc.peer_id(), 999);
    assert_tree_error(
        tree.get_meta(missing),
        &format!("Movable Tree Error: TreeID {missing:?} doesn't exist"),
    );
    assert_tree_error(
        tree.delete(missing),
        &format!("Movable Tree Error: TreeID {missing:?} is deleted or does not exist"),
    );

    tree.delete(child_a)?;
    assert_eq!(tree.parent(child_a), Some(TreeParentId::Deleted));
    assert_eq!(tree.parent(grandchild), Some(TreeParentId::Node(child_a)));
    assert_eq!(tree.children(root), Some(vec![child_b]));
    assert_eq!(tree.children_num(root), Some(1));
    assert_eq!(tree.children(child_a), Some(vec![grandchild]));
    assert_eq!(tree.children_num(child_a), Some(1));

    let after_delete_live_nodes = tree.get_nodes(false);
    let after_delete_all_nodes = tree.get_nodes(true);
    assert_eq!(
        after_delete_live_nodes
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![root, child_b]
    );
    assert_eq!(
        after_delete_all_nodes
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![root, child_b, child_a, grandchild]
    );
    assert_eq!(after_delete_all_nodes[2].parent, TreeParentId::Deleted);
    assert_eq!(
        after_delete_all_nodes[3].parent,
        TreeParentId::Node(child_a)
    );

    doc.commit();

    let after_delete_value = tree.get_value_with_meta().to_json_value();
    let after_delete_live_summary = summarize_nodes(&after_delete_live_nodes);
    let after_delete_all_summary = summarize_nodes(&after_delete_all_nodes);
    let after_delete_snapshot = doc.export(ExportMode::Snapshot)?;
    let after_delete_restored = LoroDoc::from_snapshot(&after_delete_snapshot)?;
    let after_delete_tree = after_delete_restored.get_tree("outline");

    assert_eq!(deep_json(&after_delete_restored), deep_json(&doc));
    assert_eq!(
        after_delete_tree.get_value_with_meta().to_json_value(),
        after_delete_value
    );
    assert_eq!(
        summarize_nodes(&after_delete_tree.get_nodes(false)),
        after_delete_live_summary
    );
    assert_eq!(
        summarize_nodes(&after_delete_tree.get_nodes(true)),
        after_delete_all_summary
    );
    assert_eq!(after_delete_tree.children(root), Some(vec![child_b]));
    assert_eq!(after_delete_tree.children_num(root), Some(1));
    assert_eq!(after_delete_tree.children(child_a), Some(vec![grandchild]));
    assert_eq!(after_delete_tree.children_num(child_a), Some(1));
    assert_eq!(
        after_delete_tree.parent(child_a),
        Some(TreeParentId::Deleted)
    );
    assert_eq!(
        after_delete_tree.parent(grandchild),
        Some(TreeParentId::Node(child_a))
    );

    Ok(())
}

#[test]
fn attached_tree_create_at_rejects_out_of_bounds_index() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;

    let tree = doc.get_tree("outline");
    tree.enable_fractional_index(0);

    let root = tree.create(TreeParentId::Root)?;
    let _child_a = tree.create_at(root, 0)?;
    let _child_b = tree.create_at(root, 1)?;

    assert_tree_error(
        tree.create_at(root, 3),
        "Movable Tree Error: The index(3) should be <= the length of children (2)",
    );

    Ok(())
}

#[test]
fn detached_tree_create_at_inserts_at_the_requested_position() -> LoroResult<()> {
    let tree = LoroTree::new();
    let root = tree.create(TreeParentId::Root)?;
    let tail = tree.create(root)?;
    let head = tree.create_at(root, 0)?;

    assert_eq!(tree.children(root), Some(vec![head, tail]));
    assert_eq!(tree.children_num(root), Some(2));
    assert_eq!(tree.parent(head), Some(TreeParentId::Node(root)));
    assert_eq!(tree.parent(tail), Some(TreeParentId::Node(root)));
    assert_eq!(tree.fractional_index(head).as_deref(), Some("80"));
    assert_eq!(tree.fractional_index(tail).as_deref(), Some("80"));

    Ok(())
}

#[test]
fn detached_tree_create_move_delete_and_reset_state_stay_local() -> LoroResult<()> {
    let tree = LoroTree::new();

    assert!(!tree.is_attached());
    assert!(tree.doc().is_none());
    assert!(tree.get_attached().is_none());
    assert!(tree.is_empty());

    let root = tree.create(TreeParentId::Root)?;
    let child_a = tree.create(root)?;
    let child_b = tree.create(root)?;

    assert_eq!(tree.children(TreeParentId::Root), Some(vec![root]));
    assert_eq!(tree.children(root), Some(vec![child_a, child_b]));
    assert_eq!(tree.children_num(root), Some(2));
    assert_eq!(tree.fractional_index(root).as_deref(), Some("80"));
    assert_eq!(tree.fractional_index(child_a).as_deref(), Some("80"));
    assert_eq!(tree.fractional_index(child_b).as_deref(), Some("80"));

    let root_meta = tree.get_meta(root)?;
    root_meta.insert("title", "root")?;
    tree.get_meta(child_a)?.insert("title", "child-a")?;

    tree.mov(child_b, TreeParentId::Root)?;
    assert_eq!(tree.parent(child_b), Some(TreeParentId::Root));
    assert_eq!(tree.roots(), vec![root, child_b]);
    assert_eq!(tree.children(root), Some(vec![child_a]));
    assert_eq!(tree.children_num(root), Some(1));
    assert_eq!(tree.fractional_index(child_b).as_deref(), Some("80"));

    tree.delete(child_a)?;
    assert!(!tree.contains(child_a));
    assert_eq!(tree.parent(child_a), None);
    assert_eq!(tree.children(root), Some(vec![]));
    assert_eq!(tree.children_num(root), Some(0));
    assert_eq!(tree.fractional_index(child_a), None);
    assert_eq!(tree.fractional_index(root).as_deref(), Some("80"));

    tree.delete(child_b)?;
    tree.delete(root)?;
    assert!(tree.is_empty());
    assert_eq!(tree.roots(), Vec::<TreeID>::new());
    assert_eq!(tree.children(TreeParentId::Root), Some(vec![]));
    assert_eq!(tree.children_num(TreeParentId::Root), Some(0));
    assert_eq!(tree.fractional_index(root), None);
    assert_eq!(tree.get_value_with_meta().to_json_value(), json!([]));

    let fresh_root = tree.create(TreeParentId::Root)?;
    let fresh_child = tree.create(fresh_root)?;
    assert_eq!(tree.children(fresh_root), Some(vec![fresh_child]));
    assert_eq!(tree.fractional_index(fresh_root).as_deref(), Some("80"));
    assert_eq!(tree.fractional_index(fresh_child).as_deref(), Some("80"));

    Ok(())
}
