use loro::{ExportMode, LoroDoc, LoroResult, ToJson, TreeParentId};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

#[test]
fn concurrent_delete_ancestor_and_move_descendant_out_keeps_moved_node_alive() -> LoroResult<()> {
    let base = LoroDoc::new();
    base.set_peer_id(701)?;
    let tree = base.get_tree("outline");
    tree.enable_fractional_index(0);

    let root = tree.create(TreeParentId::Root)?;
    let keep = tree.create_at(root, 0)?;
    let deleted_parent = tree.create_at(root, 1)?;
    let moved_child = tree.create_at(deleted_parent, 0)?;
    tree.get_meta(root)?.insert("name", "root")?;
    tree.get_meta(keep)?.insert("name", "keep")?;
    tree.get_meta(deleted_parent)?.insert("name", "delete-me")?;
    tree.get_meta(moved_child)?.insert("name", "move-me")?;
    base.commit();

    let base_updates = base.export(ExportMode::all_updates())?;
    let base_vv = base.oplog_vv();

    let alice = LoroDoc::new();
    alice.set_peer_id(702)?;
    alice.import(&base_updates)?;
    let alice_tree = alice.get_tree("outline");
    alice_tree.delete(deleted_parent)?;
    alice_tree.get_meta(keep)?.insert("actor", "alice")?;
    alice.commit();
    let alice_updates = alice.export(ExportMode::updates(&base_vv))?;

    let bob = LoroDoc::new();
    bob.set_peer_id(703)?;
    bob.import(&base_updates)?;
    let bob_tree = bob.get_tree("outline");
    bob_tree.mov_to(moved_child, keep, 0)?;
    bob_tree.get_meta(moved_child)?.insert("actor", "bob")?;
    bob.commit();
    let bob_updates = bob.export(ExportMode::updates(&base_vv))?;

    alice.import(&bob_updates)?;
    bob.import(&alice_updates)?;
    assert_eq!(deep_json(&alice), deep_json(&bob));

    let merged_tree = alice.get_tree("outline");
    assert_eq!(merged_tree.parent(root), Some(TreeParentId::Root));
    assert_eq!(merged_tree.parent(keep), Some(TreeParentId::Node(root)));
    assert_eq!(
        merged_tree.parent(deleted_parent),
        Some(TreeParentId::Deleted)
    );
    assert!(merged_tree.is_node_deleted(&deleted_parent)?);
    assert!(!merged_tree.is_node_deleted(&moved_child)?);
    assert_eq!(
        merged_tree.parent(moved_child),
        Some(TreeParentId::Node(keep))
    );
    assert_eq!(merged_tree.children(root), Some(vec![keep]));
    assert_eq!(merged_tree.children(keep), Some(vec![moved_child]));
    assert!(merged_tree
        .get_nodes(false)
        .iter()
        .all(|node| node.id != deleted_parent));
    assert!(merged_tree
        .get_nodes(true)
        .iter()
        .any(|node| node.id == deleted_parent));
    assert_eq!(
        merged_tree
            .get_meta(moved_child)?
            .get("actor")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("bob")
    );

    let restored = LoroDoc::from_snapshot(&alice.export(ExportMode::Snapshot)?)?;
    assert_eq!(deep_json(&restored), deep_json(&alice));
    let restored_tree = restored.get_tree("outline");
    assert_eq!(
        restored_tree.parent(deleted_parent),
        Some(TreeParentId::Deleted)
    );
    assert_eq!(
        restored_tree.parent(moved_child),
        Some(TreeParentId::Node(keep))
    );

    Ok(())
}
