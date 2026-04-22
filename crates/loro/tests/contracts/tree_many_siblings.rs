use loro::{ExportMode, LoroDoc, LoroResult, ToJson, TreeID, TreeParentId};
use pretty_assertions::assert_eq;
use serde_json::json;

fn move_after(order: &mut Vec<TreeID>, target: TreeID, after: TreeID) {
    let target_index = order.iter().position(|id| *id == target).unwrap();
    let target = order.remove(target_index);
    let after_index = order.iter().position(|id| *id == after).unwrap();
    order.insert(after_index + 1, target);
}

fn move_before(order: &mut Vec<TreeID>, target: TreeID, before: TreeID) {
    let target_index = order.iter().position(|id| *id == target).unwrap();
    let target = order.remove(target_index);
    let before_index = order.iter().position(|id| *id == before).unwrap();
    order.insert(before_index, target);
}

#[test]
fn tree_many_siblings_keep_order_positions_and_snapshot_contracts() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(601)?;
    let tree = doc.get_tree("outline");
    tree.enable_fractional_index(0);

    let root = tree.create(TreeParentId::Root)?;
    tree.get_meta(root)?.insert("label", "root")?;

    let mut children = Vec::new();
    for i in 0..24 {
        let child = tree.create_at(root, i)?;
        tree.get_meta(child)?.insert("index", i as i64)?;
        children.push(child);
    }
    assert_eq!(tree.children(root), Some(children.clone()));
    assert_eq!(tree.children_num(root), Some(children.len()));
    assert_eq!(tree.roots(), vec![root]);
    assert!(children
        .iter()
        .all(|id| tree.fractional_index(*id).is_some()));

    let last = *children.last().unwrap();
    tree.mov_to(last, root, 0)?;
    children.pop();
    children.insert(0, last);
    assert_eq!(tree.children(root), Some(children.clone()));

    let target = children[8];
    let after = children[2];
    tree.mov_after(target, after)?;
    move_after(&mut children, target, after);
    assert_eq!(tree.children(root), Some(children.clone()));

    let target = children[12];
    let before = children[4];
    tree.mov_before(target, before)?;
    move_before(&mut children, target, before);
    assert_eq!(tree.children(root), Some(children.clone()));

    let deleted = children.remove(10);
    tree.delete(deleted)?;
    assert_eq!(tree.is_node_deleted(&deleted), Ok(true));
    assert_eq!(tree.parent(deleted), Some(TreeParentId::Deleted));
    assert_eq!(tree.children(root), Some(children.clone()));

    let nodes_without_deleted = tree.get_nodes(false);
    assert_eq!(nodes_without_deleted.len(), 1 + children.len());
    assert!(nodes_without_deleted
        .iter()
        .filter(|node| node.parent == TreeParentId::Node(root))
        .all(|node| children.contains(&node.id)));

    let nodes_with_deleted = tree.get_nodes(true);
    assert!(nodes_with_deleted.iter().any(|node| node.id == deleted));

    let root_meta = tree.get_meta(root)?;
    root_meta.insert("child_count", children.len() as i64)?;
    assert_eq!(
        tree.get_meta(root)?
            .get("child_count")
            .unwrap()
            .get_deep_value(),
        (children.len() as i64).into()
    );

    let snapshot = doc.export(ExportMode::Snapshot)?;
    let restored = LoroDoc::from_snapshot(&snapshot)?;
    assert_eq!(
        restored.get_deep_value().to_json_value(),
        doc.get_deep_value().to_json_value()
    );
    assert_eq!(
        restored.get_tree("outline").children(root),
        Some(children.clone())
    );

    Ok(())
}

#[test]
fn tree_fractional_index_guardrails_are_enforced() -> LoroResult<()> {
    let doc = LoroDoc::new();
    let tree = doc.get_tree("guarded");
    assert!(tree.is_fractional_index_enabled());
    let root = tree.create(TreeParentId::Root)?;

    tree.disable_fractional_index();
    assert!(!tree.is_fractional_index_enabled());
    assert!(tree.create_at(root, 0).is_err());
    assert!(tree.mov_to(root, TreeParentId::Root, 0).is_err());
    assert!(tree.mov_after(root, root).is_err());
    assert!(tree.mov_before(root, root).is_err());
    assert_eq!(tree.children(TreeParentId::Root), Some(vec![root]));

    tree.enable_fractional_index(0);
    assert!(tree.is_fractional_index_enabled());
    let child = tree.create_at(root, 0)?;
    assert_eq!(tree.children(root), Some(vec![child]));
    assert!(tree.fractional_index(child).is_some());

    tree.disable_fractional_index();
    assert!(!tree.is_fractional_index_enabled());
    let appended = tree.create(root)?;
    assert_eq!(tree.children(root), Some(vec![child, appended]));
    assert_eq!(
        tree.get_value_with_meta().to_json_value()[0]["children"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let deep_value = doc.get_deep_value().to_json_value();
    let roots = deep_value["guarded"].as_array().unwrap();
    assert_eq!(roots.len(), 1);

    let root_node = &roots[0];
    assert_eq!(root_node["id"], json!(root.to_string()));
    assert_eq!(root_node["parent"], json!(null));
    assert_eq!(root_node["index"], json!(0));

    let child_nodes = root_node["children"].as_array().unwrap();
    assert_eq!(child_nodes.len(), 2);
    for (index, id) in [child, appended].into_iter().enumerate() {
        assert_eq!(child_nodes[index]["id"], json!(id.to_string()));
        assert_eq!(child_nodes[index]["parent"], json!(root.to_string()));
        assert_eq!(child_nodes[index]["index"], json!(index));
        assert_eq!(child_nodes[index]["children"], json!([]));
    }

    Ok(())
}

#[test]
fn tree_reenabled_fractional_index_positions_after_unpositioned_inserts() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(602)?;
    let tree = doc.get_tree("outline");
    let root = tree.create(TreeParentId::Root)?;

    tree.disable_fractional_index();
    let first = tree.create(root)?;
    let second = tree.create(root)?;
    let third = tree.create(root)?;
    assert_eq!(tree.children(root), Some(vec![first, second, third]));

    tree.enable_fractional_index(0);
    let inserted = tree.create_at(root, 1)?;
    assert_eq!(
        tree.children(root),
        Some(vec![first, inserted, second, third])
    );
    assert!(tree.fractional_index(inserted).is_some());

    tree.mov_to(third, root, 0)?;
    assert_eq!(
        tree.children(root),
        Some(vec![third, first, inserted, second])
    );

    let restored = LoroDoc::from_snapshot(&doc.export(ExportMode::Snapshot)?)?;
    assert_eq!(
        restored.get_tree("outline").children(root),
        Some(vec![third, first, inserted, second])
    );

    Ok(())
}

#[test]
fn tree_fractional_index_jitter_rebalances_dense_siblings_and_survives_snapshot() -> LoroResult<()>
{
    let doc = LoroDoc::new();
    doc.set_peer_id(603)?;
    let tree = doc.get_tree("outline");
    let root = tree.create(TreeParentId::Root)?;

    tree.disable_fractional_index();
    let mut children = Vec::new();
    for _ in 0..18 {
        children.push(tree.create(root)?);
    }
    assert_eq!(tree.children(root), Some(children.clone()));

    tree.enable_fractional_index(7);
    let inserted = tree.create_at(root, 9)?;
    children.insert(9, inserted);
    assert_eq!(tree.children(root), Some(children.clone()));
    assert!(tree
        .children(root)
        .unwrap()
        .iter()
        .all(|id| tree.fractional_index(*id).is_some()));

    let anchor = children[4];
    let moved_id = children[13];
    tree.mov_before(moved_id, anchor)?;
    let moved = children.remove(13);
    let before = children.iter().position(|id| *id == anchor).unwrap();
    children.insert(before, moved);
    assert_eq!(tree.children(root), Some(children.clone()));

    let snapshot = doc.export(ExportMode::Snapshot)?;
    let restored = LoroDoc::from_snapshot(&snapshot)?;
    assert_eq!(
        restored.get_tree("outline").children(root),
        Some(children.clone())
    );
    assert_eq!(
        restored.get_tree("outline").children(root).unwrap().len(),
        19
    );

    Ok(())
}
