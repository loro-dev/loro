use loro_common::{LoroResult, PeerID};
use loro_internal::{handler::TreeHandler, LoroDoc, ToJson, TreeID, TreeParentId};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

#[test]
fn detached_tree_handler_queries_and_clear_follow_contracts() -> LoroResult<()> {
    let tree = TreeHandler::new_detached();
    assert_eq!(format!("{tree:?}"), "TreeHandler Detached");
    assert!(tree.is_fractional_index_enabled());
    tree.enable_fractional_index(2);
    tree.disable_fractional_index();
    assert!(!tree.is_deleted());
    assert!(tree.is_empty());
    assert_eq!(tree.__internal__next_tree_id(), TreeID::new(PeerID::MAX, 0));

    let root = tree.create(TreeParentId::Root)?;
    let first = tree.create_at(root.into(), 0)?;
    let second = tree.create_at(root.into(), 1)?;
    let third = tree.create_at(root.into(), 2)?;

    assert_eq!(tree.__internal__next_tree_id(), TreeID::new(PeerID::MAX, 4));
    assert_eq!(tree.get_child_at(&TreeParentId::Root, 0), Some(root));
    assert_eq!(tree.get_child_at(&root.into(), 0), Some(first));
    assert_eq!(tree.get_child_at(&root.into(), 1), Some(second));
    assert_eq!(tree.get_child_at(&root.into(), 2), Some(third));
    assert_eq!(tree.get_child_at(&root.into(), 3), None);
    assert_eq!(tree.get_index_by_tree_id(&second), Some(1));
    assert!(tree.get_position_by_tree_id(&second).is_some());
    assert!(tree
        .get_position_by_tree_id(&TreeID::new(PeerID::MAX, 99))
        .is_none());
    assert!(tree.is_parent(&second, &root.into()));
    assert_eq!(tree.get_nodes_under(TreeParentId::Root).len(), 4);
    assert_eq!(tree.get_nodes_under(root.into()).len(), 3);
    assert_eq!(tree.nodes().len(), 4);

    tree.mov_before(third, first)?;
    assert_eq!(
        tree.children(&root.into()),
        Some(vec![third, first, second])
    );
    tree.mov_after(third, second)?;
    assert_eq!(
        tree.children(&root.into()),
        Some(vec![first, second, third])
    );
    tree.move_to(third, TreeParentId::Root, 0)?;
    assert_eq!(tree.children(&TreeParentId::Root), Some(vec![third, root]));

    tree.delete(third)?;
    assert!(!tree.contains(third));
    assert_eq!(tree.get_node_parent(&third), None);

    tree.clear()?;
    assert!(tree.is_empty());
    assert_eq!(tree.roots(), Vec::<TreeID>::new());
    assert_eq!(tree.nodes(), Vec::<TreeID>::new());

    Ok(())
}

#[test]
fn attached_tree_hierarchy_positions_and_diff_apply_follow_contracts() -> LoroResult<()> {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(77)?;
    let tree = doc.get_tree("outline");
    tree.enable_fractional_index(2);

    let root = tree.create(TreeParentId::Root)?;
    let first = tree.create_at(root.into(), 0)?;
    let second = tree.create_at(root.into(), 1)?;
    tree.get_meta(root)?.insert("name", "root")?;
    tree.get_meta(first)?.insert("name", "first")?;
    tree.get_meta(second)?.insert("name", "second")?;

    assert!(format!("{tree:?}").contains("TreeHandler"));
    assert_eq!(tree.get_child_at(&TreeParentId::Root, 0), Some(root));
    assert_eq!(tree.get_child_at(&root.into(), 0), Some(first));
    assert_eq!(tree.get_child_at(&root.into(), 1), Some(second));
    assert_eq!(tree.get_index_by_tree_id(&second), Some(1));
    assert!(tree.get_position_by_tree_id(&second).is_some());
    assert!(tree.get_last_move_id(&root).is_some());

    let hierarchy = tree.get_all_hierarchy_nodes_under(TreeParentId::Root);
    assert_eq!(hierarchy.len(), 1);
    assert_eq!(hierarchy[0].id, root);
    assert_eq!(hierarchy[0].children.len(), 2);
    assert_eq!(hierarchy[0].children[0].id, first);
    assert_eq!(hierarchy[0].children[1].id, second);

    doc.commit_then_renew();
    let v1 = doc.state_frontiers();
    let before = deep_json(&doc);

    tree.move_to(second, TreeParentId::Root, 0)?;
    tree.delete(first)?;
    tree.get_meta(second)?.insert("status", "promoted")?;
    doc.commit_then_renew();
    let v2 = doc.state_frontiers();
    let after = deep_json(&doc);
    assert_ne!(before, after);

    let fork = doc.fork_at(&v1)?;
    assert_eq!(deep_json(&fork), before);
    fork.apply_diff(doc.diff(&v1, &v2)?)?;
    assert_eq!(deep_json(&fork), after);

    tree.clear()?;
    doc.commit_then_renew();
    assert!(tree.is_empty());
    assert_eq!(deep_json(&doc), json!({"outline": []}));

    Ok(())
}
