use loro::{ExportMode, LoroDoc, LoroList, LoroMap, LoroResult, LoroText, ToJson, TreeParentId};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

fn assert_tree_error<T: std::fmt::Debug>(result: LoroResult<T>, expected: &str) {
    let err = match result {
        Ok(value) => panic!("expected tree operation to fail, got {value:?}"),
        Err(err) => err,
    };
    assert_eq!(err.to_string(), expected);
}

#[test]
fn tree_lifecycle_queries_and_deleted_parent_contracts() -> LoroResult<()> {
    let doc = LoroDoc::new();
    let tree = doc.get_tree("outline");

    assert!(tree.is_empty());
    assert!(tree.is_fractional_index_enabled());
    assert_eq!(tree.roots(), vec![]);
    assert_eq!(tree.children(TreeParentId::Root), None);
    assert_eq!(tree.children_num(TreeParentId::Root), None);

    let root = tree.create(TreeParentId::Root)?;
    let child_a = tree.create(root)?;
    let child_b = tree.create(root)?;
    let grandchild = tree.create(child_a)?;

    assert!(!tree.is_empty());
    assert_eq!(tree.roots(), vec![root]);
    assert_eq!(tree.children(TreeParentId::Root), Some(vec![root]));
    assert_eq!(tree.children(root), Some(vec![child_a, child_b]));
    assert_eq!(tree.children(child_a), Some(vec![grandchild]));
    assert_eq!(tree.children_num(root), Some(2));
    assert_eq!(tree.children_num(child_a), Some(1));
    assert_eq!(tree.contains(root), true);
    assert_eq!(tree.contains(child_a), true);
    assert_eq!(tree.contains(child_b), true);
    assert_eq!(tree.contains(grandchild), true);
    assert_eq!(tree.parent(root), Some(TreeParentId::Root));
    assert_eq!(tree.parent(child_a), Some(TreeParentId::Node(root)));
    assert_eq!(tree.parent(child_b), Some(TreeParentId::Node(root)));
    assert_eq!(tree.parent(grandchild), Some(TreeParentId::Node(child_a)));
    assert_eq!(tree.is_node_deleted(&root)?, false);
    assert_eq!(tree.is_node_deleted(&child_a)?, false);
    assert!(tree.fractional_index(root).is_some());
    assert!(tree.get_last_move_id(&root).is_some());

    let nodes = tree.get_nodes(false);
    assert_eq!(nodes.len(), 4);
    assert_eq!(nodes[0].id, root);
    assert_eq!(nodes[0].parent, TreeParentId::Root);
    assert_eq!(nodes[0].index, 0);
    assert_eq!(nodes[1].id, child_a);
    assert_eq!(nodes[1].parent, TreeParentId::Node(root));
    assert_eq!(nodes[1].index, 0);
    assert_eq!(nodes[2].id, child_b);
    assert_eq!(nodes[2].parent, TreeParentId::Node(root));
    assert_eq!(nodes[2].index, 1);
    assert_eq!(nodes[3].id, grandchild);
    assert_eq!(nodes[3].parent, TreeParentId::Node(child_a));
    assert_eq!(nodes[3].index, 0);

    let meta = tree.get_meta(root)?;
    let nested = meta.insert_container("nested", LoroMap::new())?;
    nested.insert("label", "root-meta")?;
    let children = meta.insert_container("children", LoroList::new())?;
    children.push("one")?;
    children.push("two")?;

    let root_node = nodes.iter().find(|n| n.id == root).unwrap();
    let child_a_node = nodes.iter().find(|n| n.id == child_a).unwrap();
    let child_b_node = nodes.iter().find(|n| n.id == child_b).unwrap();
    let grandchild_node = nodes.iter().find(|n| n.id == grandchild).unwrap();

    assert_eq!(
        tree.get_value_with_meta().to_json_value(),
        json!([
            {
                "id": root.to_string(),
                "parent": null,
                "meta": {
                    "children": ["one", "two"],
                    "nested": {"label": "root-meta"}
                },
                "fractional_index": root_node.fractional_index.to_string(),
                "index": 0,
                "children": [
                    {
                        "id": child_a.to_string(),
                        "parent": root.to_string(),
                        "meta": {},
                        "fractional_index": child_a_node.fractional_index.to_string(),
                        "index": 0,
                        "children": [
                            {
                                "id": grandchild.to_string(),
                                "parent": child_a.to_string(),
                                "meta": {},
                                "fractional_index": grandchild_node.fractional_index.to_string(),
                                "index": 0,
                                "children": []
                            }
                        ]
                    },
                    {
                        "id": child_b.to_string(),
                        "parent": root.to_string(),
                        "meta": {},
                        "fractional_index": child_b_node.fractional_index.to_string(),
                        "index": 1,
                        "children": []
                    }
                ]
            }
        ])
    );

    assert_tree_error(
        tree.create(TreeParentId::Deleted),
        "Movable Tree Error: The provided parent id is invalid",
    );
    assert_tree_error(
        tree.create(TreeParentId::Unexist),
        "Movable Tree Error: The provided parent id is invalid",
    );

    tree.delete(child_a)?;
    assert_eq!(tree.is_node_deleted(&child_a)?, true);
    assert_eq!(tree.parent(child_a), Some(TreeParentId::Deleted));
    assert_eq!(tree.children(child_a), Some(vec![grandchild]));
    assert_eq!(tree.children_num(child_a), Some(1));
    assert!(tree.contains(child_a));
    assert!(tree.is_node_deleted(&grandchild).is_ok());
    assert_eq!(tree.parent(grandchild), Some(TreeParentId::Node(child_a)));
    assert_eq!(tree.get_nodes(true).iter().any(|n| n.id == child_a), true);
    assert_eq!(
        tree.get_nodes(true).iter().any(|n| n.id == grandchild),
        true
    );
    assert_eq!(
        tree.get_nodes(true)
            .iter()
            .find(|n| n.id == child_a)
            .map(|n| n.parent),
        Some(TreeParentId::Deleted)
    );

    assert_tree_error(
        tree.delete(child_a),
        &format!("Movable Tree Error: TreeID {child_a:?} is deleted or does not exist"),
    );
    let future = tree.__internal__next_tree_id();
    assert_tree_error(
        tree.is_node_deleted(&future),
        &format!("Movable Tree Error: TreeID {future:?} doesn't exist"),
    );

    Ok(())
}

#[test]
fn tree_fractional_index_and_move_contracts_follow_docs() -> LoroResult<()> {
    let doc = LoroDoc::new();
    let tree = doc.get_tree("board");
    let root = tree.create(TreeParentId::Root)?;
    let sibling_a = tree.create(root)?;
    let sibling_b = tree.create(root)?;

    assert!(tree.is_fractional_index_enabled());
    assert!(tree.fractional_index(root).is_some());

    tree.disable_fractional_index();
    assert!(!tree.is_fractional_index_enabled());
    assert_tree_error(
        tree.mov_to(sibling_b, root, 0),
        "Movable Tree Error: Fractional index is not enabled, you should enable it first by `LoroTree::set_enable_fractional_index`",
    );
    assert_tree_error(
        tree.mov_before(sibling_b, sibling_a),
        "Movable Tree Error: Fractional index is not enabled, you should enable it first by `LoroTree::set_enable_fractional_index`",
    );
    assert_tree_error(
        tree.mov_after(sibling_a, sibling_b),
        "Movable Tree Error: Fractional index is not enabled, you should enable it first by `LoroTree::set_enable_fractional_index`",
    );

    tree.enable_fractional_index(2);
    assert!(tree.is_fractional_index_enabled());

    let inserted = tree.create_at(root, 1)?;
    assert_eq!(
        tree.children(root),
        Some(vec![sibling_a, inserted, sibling_b])
    );
    assert_eq!(tree.children_num(root), Some(3));
    assert!(tree.fractional_index(inserted).is_some());

    tree.mov_before(sibling_b, sibling_a)?;
    assert_eq!(
        tree.children(root),
        Some(vec![sibling_b, sibling_a, inserted])
    );
    assert!(tree.get_last_move_id(&sibling_b).is_some());

    tree.mov_after(inserted, sibling_b)?;
    assert_eq!(
        tree.children(root),
        Some(vec![sibling_b, inserted, sibling_a])
    );
    assert!(tree.get_last_move_id(&inserted).is_some());

    tree.mov_to(sibling_a, root, 0)?;
    assert_eq!(
        tree.children(root),
        Some(vec![sibling_a, sibling_b, inserted])
    );
    assert_tree_error(
        tree.mov_to(inserted, root, 4),
        "Movable Tree Error: The index(4) should be <= the length of children (2)",
    );

    tree.disable_fractional_index();
    assert!(!tree.is_fractional_index_enabled());
    assert_tree_error(
        tree.create_at(root, 0),
        "Movable Tree Error: Fractional index is not enabled, you should enable it first by `LoroTree::set_enable_fractional_index`",
    );
    assert_tree_error(
        tree.mov_to(sibling_b, root, 2),
        "Movable Tree Error: Fractional index is not enabled, you should enable it first by `LoroTree::set_enable_fractional_index`",
    );

    tree.enable_fractional_index(2);
    assert!(tree.is_fractional_index_enabled());
    let moved = tree.create_at(root, 3)?;
    assert_eq!(tree.children_num(root), Some(4));
    assert!(tree.fractional_index(moved).is_some());

    tree.delete(moved)?;
    assert_tree_error(
        tree.mov(moved, root),
        &format!("Movable Tree Error: TreeID {moved:?} is deleted or does not exist"),
    );
    assert_tree_error(
        tree.mov_before(moved, sibling_a),
        &format!("Movable Tree Error: TreeID {moved:?} is deleted or does not exist"),
    );
    assert_tree_error(
        tree.mov_after(moved, sibling_b),
        &format!("Movable Tree Error: TreeID {moved:?} is deleted or does not exist"),
    );

    assert_eq!(tree.is_node_deleted(&moved)?, true);

    Ok(())
}

#[test]
fn tree_snapshot_import_checkout_revert_and_apply_diff_roundtrip() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(41)?;
    let tree = doc.get_tree("timeline");
    tree.enable_fractional_index(0);

    let root = tree.create(TreeParentId::Root)?;
    let phase_1 = tree.create_at(root, 0)?;
    let phase_2 = tree.create_at(root, 1)?;
    let phase_3 = tree.create_at(root, 2)?;

    tree.get_meta(root)?.insert("title", "root")?;
    let root_meta = tree.get_meta(root)?;
    let info = root_meta.insert_container("info", LoroMap::new())?;
    info.insert("owner", "ops")?;
    let tags = root_meta.insert_container("tags", LoroList::new())?;
    tags.push("alpha")?;
    tags.push("beta")?;

    let phase_1_meta = tree.get_meta(phase_1)?;
    let content = phase_1_meta.insert_container("content", LoroText::new())?;
    content.insert(0, "draft")?;

    doc.commit();
    let v1 = doc.state_frontiers();
    let snapshot_v1 = doc.export(ExportMode::Snapshot)?;
    let value_v1 = deep_json(&doc);
    let updates_v1 = doc.export(ExportMode::all_updates())?;

    tree.mov_before(phase_3, phase_1)?;
    tree.delete(phase_2)?;
    tree.get_meta(root)?.insert("title", "root-v2")?;
    let details = tree
        .get_meta(phase_3)?
        .insert_container("details", LoroMap::new())?;
    details.insert("status", "done")?;
    let notes = tree
        .get_meta(phase_3)?
        .insert_container("notes", LoroList::new())?;
    notes.push("handoff")?;
    doc.commit();
    let v2 = doc.state_frontiers();
    let snapshot_v2 = doc.export(ExportMode::Snapshot)?;
    let value_v2 = deep_json(&doc);

    assert_eq!(tree.children(root), Some(vec![phase_3, phase_1]));
    assert_eq!(tree.parent(phase_2), Some(TreeParentId::Deleted));
    assert_eq!(tree.is_node_deleted(&phase_2)?, true);
    assert_eq!(
        tree.get_meta(phase_3)?
            .get("details")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!({"status": "done"})
    );
    assert_eq!(
        tree.get_meta(phase_3)?
            .get("notes")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!(["handoff"])
    );

    let restored_v1 = LoroDoc::from_snapshot(&snapshot_v1)?;
    assert_eq!(deep_json(&restored_v1), value_v1);
    let restored_v1_tree = restored_v1.get_tree("timeline");
    let restored_v1_root = restored_v1_tree.roots()[0];
    assert_eq!(restored_v1_tree.children_num(restored_v1_root), Some(3));
    assert_eq!(
        restored_v1_tree
            .get_meta(restored_v1_root)?
            .get("title")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("root")
    );

    let restored_v2 = LoroDoc::from_snapshot(&snapshot_v2)?;
    assert_eq!(deep_json(&restored_v2), value_v2);
    let restored_v2_tree = restored_v2.get_tree("timeline");
    let restored_v2_root = restored_v2_tree.roots()[0];
    assert_eq!(restored_v2_tree.children_num(restored_v2_root), Some(2));
    assert_eq!(
        restored_v2_tree
            .get_meta(restored_v2_root)?
            .get("title")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("root-v2")
    );

    let diff = doc.diff(&v1, &v2)?;
    let patched = LoroDoc::new();
    patched.import(&updates_v1)?;
    let patched_tree = patched.get_tree("timeline");
    assert_eq!(
        patched_tree.children(root).map(|children| children.len()),
        Some(3)
    );
    assert_eq!(
        patched_tree
            .get_meta(root)?
            .get("title")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("root")
    );
    patched.apply_diff(diff)?;
    let patched_tree = patched.get_tree("timeline");
    let patched_children = patched_tree.children(root).unwrap();
    assert_eq!(patched_children.len(), 2);
    assert_eq!(
        patched_tree
            .get_meta(root)?
            .get("title")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("root-v2")
    );
    let mut found_details = false;
    let mut found_content = false;
    for child in patched_children {
        let meta = patched_tree.get_meta(child)?;
        if meta.get("details").is_some() {
            assert_eq!(
                meta.get("details")
                    .unwrap()
                    .get_deep_value()
                    .to_json_value(),
                json!({"status": "done"})
            );
            assert_eq!(
                meta.get("notes").unwrap().get_deep_value().to_json_value(),
                json!(["handoff"])
            );
            found_details = true;
        }
        if meta.get("content").is_some() {
            assert_eq!(
                meta.get("content")
                    .unwrap()
                    .get_deep_value()
                    .to_json_value(),
                json!("draft")
            );
            found_content = true;
        }
    }
    assert!(found_details);
    assert!(found_content);

    let reverted = LoroDoc::from_snapshot(&snapshot_v2)?;
    reverted.revert_to(&v1)?;
    let reverted_tree = reverted.get_tree("timeline");
    let reverted_root = reverted_tree.roots()[0];
    assert_eq!(reverted_tree.children_num(reverted_root), Some(3));
    assert_eq!(
        reverted_tree
            .get_meta(reverted_root)?
            .get("title")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("root")
    );

    Ok(())
}

#[test]
fn concurrent_tree_moves_deletes_and_meta_edits_converge() -> LoroResult<()> {
    let base = LoroDoc::new();
    base.set_peer_id(51)?;
    let tree = base.get_tree("roadmap");
    tree.enable_fractional_index(0);
    let root = tree.create(TreeParentId::Root)?;
    let alpha = tree.create_at(root, 0)?;
    let beta = tree.create_at(root, 1)?;
    let gamma = tree.create_at(root, 2)?;
    tree.get_meta(root)?.insert("owner", "base")?;
    tree.get_meta(alpha)?.insert("state", "todo")?;
    tree.get_meta(beta)?.insert("state", "doing")?;
    base.commit();

    let base_updates = base.export(ExportMode::all_updates())?;
    let base_vv = base.oplog_vv();

    let alice = LoroDoc::new();
    alice.set_peer_id(52)?;
    alice.import(&base_updates)?;
    let bob = LoroDoc::new();
    bob.set_peer_id(53)?;
    bob.import(&base_updates)?;

    let alice_tree = alice.get_tree("roadmap");
    alice_tree.mov_after(alpha, beta)?;
    alice_tree.get_meta(beta)?.insert("state", "blocked")?;
    alice_tree
        .get_meta(alpha)?
        .insert_container("history", LoroList::new())?
        .push("moved-by-alice")?;
    alice.commit();
    let alice_updates = alice.export(ExportMode::updates(&base_vv))?;

    let bob_tree = bob.get_tree("roadmap");
    bob_tree.delete(beta)?;
    bob_tree.get_meta(root)?.insert("owner", "bob")?;
    bob_tree.mov_to(gamma, root, 0)?;
    bob_tree.get_meta(gamma)?.insert("state", "ready")?;
    bob.commit();
    let bob_updates = bob.export(ExportMode::updates(&base_vv))?;

    alice.import(&bob_updates)?;
    bob.import(&alice_updates)?;

    assert_eq!(deep_json(&alice), deep_json(&bob));
    assert_eq!(
        alice.get_tree("roadmap").children(root),
        Some(vec![gamma, alpha])
    );
    assert_eq!(
        alice.get_tree("roadmap").parent(beta),
        Some(TreeParentId::Deleted)
    );
    assert_eq!(alice.get_tree("roadmap").is_node_deleted(&beta)?, true);
    assert_eq!(
        alice
            .get_tree("roadmap")
            .get_meta(root)?
            .get("owner")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("bob")
    );
    assert_eq!(
        alice
            .get_tree("roadmap")
            .get_meta(alpha)?
            .get("history")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!(["moved-by-alice"])
    );
    assert!(alice.get_tree("roadmap").get_last_move_id(&alpha).is_some());
    assert!(alice.get_tree("roadmap").get_last_move_id(&gamma).is_some());

    let batch = LoroDoc::new();
    batch.set_peer_id(54)?;
    let status = batch.import_batch(&[bob_updates, alice_updates, base_updates])?;
    assert!(status.pending.is_none());
    assert_eq!(deep_json(&batch), deep_json(&alice));
    assert_eq!(
        batch.get_tree("roadmap").children(root),
        Some(vec![gamma, alpha])
    );
    assert_eq!(
        batch
            .get_tree("roadmap")
            .get_meta(gamma)?
            .get("state")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("ready")
    );

    Ok(())
}
