use loro::{
    ChangeTravelError, EncodedBlobMode, ExportMode, LoroDoc, LoroError, LoroList, LoroMap,
    LoroResult, ToJson, TreeParentId, ID,
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use std::ops::ControlFlow;

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

#[test]
fn tree_shallow_snapshot_roundtrips_and_blocks_checkout_before_the_shallow_root() -> LoroResult<()>
{
    let doc = LoroDoc::new();
    doc.set_peer_id(61)?;

    let tree = doc.get_tree("outline");
    tree.enable_fractional_index(0);

    let root = tree.create(TreeParentId::Root)?;
    let child_a = tree.create_at(root, 0)?;
    let child_b = tree.create_at(root, 1)?;
    let grandchild = tree.create(child_a)?;

    let root_meta = tree.get_meta(root)?;
    root_meta.insert("title", "v1-root")?;
    root_meta
        .insert_container("details", LoroMap::new())?
        .insert("owner", "alice")?;
    root_meta
        .insert_container("tags", LoroList::new())?
        .push("planning")?;
    tree.get_meta(child_a)?.insert("title", "v1-child-a")?;
    tree.get_meta(child_b)?.insert("title", "v1-child-b")?;
    tree.get_meta(grandchild)?
        .insert("title", "v1-grandchild")?;
    doc.commit();

    let before_shallow_frontiers = doc.state_frontiers();
    let before_shallow_tree = tree.get_value_with_meta().to_json_value();
    let before_shallow_doc = deep_json(&doc);

    tree.mov_before(child_b, child_a)?;
    tree.delete(child_a)?;
    tree.get_meta(root)?.insert("title", "v2-root")?;
    tree.get_meta(child_b)?.insert("status", "ready")?;
    doc.commit();

    let latest_frontiers = doc.state_frontiers();
    let latest_tree = tree.get_value_with_meta().to_json_value();
    let latest_doc = deep_json(&doc);

    let shallow = doc.export(ExportMode::shallow_snapshot(&latest_frontiers))?;
    let shallow_meta = LoroDoc::decode_import_blob_meta(&shallow, false)?;
    assert_eq!(shallow_meta.mode, EncodedBlobMode::ShallowSnapshot);

    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&shallow)?;
    assert!(shallow_doc.is_shallow());
    assert_eq!(shallow_doc.shallow_since_frontiers(), latest_frontiers);
    assert_eq!(deep_json(&shallow_doc), latest_doc);
    assert_eq!(
        shallow_doc
            .get_tree("outline")
            .get_value_with_meta()
            .to_json_value(),
        latest_tree
    );

    let err = shallow_doc.checkout(&before_shallow_frontiers).unwrap_err();
    assert_eq!(err, LoroError::SwitchToVersionBeforeShallowRoot);
    assert!(!shallow_doc.is_detached());
    assert_eq!(deep_json(&shallow_doc), latest_doc);
    assert_eq!(
        shallow_doc
            .get_tree("outline")
            .get_value_with_meta()
            .to_json_value(),
        latest_tree
    );

    let shallow_tree = shallow_doc.get_tree("outline");
    assert_eq!(shallow_tree.children(root), Some(vec![child_b]));
    assert_eq!(shallow_tree.parent(child_a), Some(TreeParentId::Deleted));
    assert_eq!(shallow_tree.parent(child_b), Some(TreeParentId::Node(root)));
    assert_eq!(shallow_tree.children(root), Some(vec![child_b]));

    assert_ne!(before_shallow_doc, latest_doc);
    assert_ne!(before_shallow_tree, latest_tree);
    assert_ne!(latest_tree, before_shallow_tree);

    Ok(())
}

#[test]
fn tree_import_batch_and_checkout_to_latest_preserve_divergent_history() -> LoroResult<()> {
    let base = LoroDoc::new();
    base.set_peer_id(62)?;

    let tree = base.get_tree("roadmap");
    tree.enable_fractional_index(0);

    let root = tree.create(TreeParentId::Root)?;
    let alpha = tree.create_at(root, 0)?;
    let beta = tree.create_at(root, 1)?;
    let gamma = tree.create_at(root, 2)?;

    tree.get_meta(root)?.insert("owner", "base")?;
    tree.get_meta(alpha)?.insert("state", "todo")?;
    tree.get_meta(beta)?.insert("state", "doing")?;
    tree.get_meta(gamma)?.insert("state", "done")?;
    base.commit();

    let base_frontiers = base.state_frontiers();
    let base_vv = base.oplog_vv();
    let base_updates = base.export(ExportMode::all_updates())?;

    let alice = LoroDoc::new();
    alice.set_peer_id(63)?;
    alice.import(&base_updates)?;
    let alice_tree = alice.get_tree("roadmap");
    alice_tree.mov_after(alpha, beta)?;
    alice_tree.get_meta(alpha)?.insert("owner", "alice")?;
    alice_tree
        .get_meta(alpha)?
        .insert_container("history", LoroList::new())?
        .push("moved")?;
    alice.commit();
    let alice_updates = alice.export(ExportMode::updates(&base_vv))?;

    let bob = LoroDoc::new();
    bob.set_peer_id(64)?;
    bob.import(&base_updates)?;
    let bob_tree = bob.get_tree("roadmap");
    bob_tree.delete(beta)?;
    bob_tree.mov_to(gamma, root, 0)?;
    bob_tree.get_meta(root)?.insert("owner", "bob")?;
    bob.commit();
    let bob_updates = bob.export(ExportMode::updates(&base_vv))?;

    let merged = LoroDoc::new();
    merged.set_peer_id(65)?;
    let status = merged.import_batch(&[
        bob_updates.clone(),
        alice_updates.clone(),
        base_updates.clone(),
    ])?;
    assert!(status.pending.is_none());

    let merged_tree = merged.get_tree("roadmap");
    assert_eq!(merged_tree.children(root), Some(vec![gamma, alpha]));
    assert_eq!(merged_tree.parent(beta), Some(TreeParentId::Deleted));
    assert!(merged_tree.is_node_deleted(&beta)?);
    assert_eq!(merged_tree.children(gamma), None);
    assert_eq!(
        merged_tree
            .get_meta(root)?
            .get("owner")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("bob")
    );
    assert_eq!(
        merged_tree
            .get_meta(alpha)?
            .get("owner")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("alice")
    );
    assert_eq!(
        merged_tree
            .get_meta(alpha)?
            .get("history")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!(["moved"])
    );
    assert_eq!(
        merged_tree
            .get_meta(gamma)?
            .get("state")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("done")
    );

    merged.checkout(&base_frontiers)?;
    assert!(merged.is_detached());
    assert_eq!(merged_tree.children(root), Some(vec![alpha, beta, gamma]));
    assert_eq!(merged_tree.parent(beta), Some(TreeParentId::Node(root)));
    assert_eq!(
        merged_tree
            .get_meta(root)?
            .get("owner")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("base")
    );

    merged.checkout_to_latest();
    assert!(!merged.is_detached());
    assert_eq!(merged_tree.children(root), Some(vec![gamma, alpha]));
    assert_eq!(merged_tree.parent(beta), Some(TreeParentId::Deleted));
    assert!(merged_tree.is_node_deleted(&beta)?);
    assert_eq!(
        merged_tree
            .get_meta(root)?
            .get("owner")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("bob")
    );
    Ok(())
}

#[test]
fn version_comparison_and_travel_change_ancestors_follow_contract() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(67)?;

    let map = doc.get_map("state");
    map.insert("title", "draft")?;
    doc.commit();
    let v1 = doc.state_frontiers();
    let v1_id = v1.iter().next().unwrap();

    map.insert("count", 1)?;
    doc.commit();
    let v2 = doc.state_frontiers();

    assert_eq!(
        doc.cmp_frontiers(&v1, &v1),
        Ok(Some(std::cmp::Ordering::Equal))
    );
    assert_eq!(
        doc.cmp_frontiers(&v1, &v2),
        Ok(Some(std::cmp::Ordering::Less))
    );
    assert_eq!(
        doc.cmp_frontiers(&v2, &v1),
        Ok(Some(std::cmp::Ordering::Greater))
    );

    let mut noop = |_| ControlFlow::Continue(());
    let err = doc
        .travel_change_ancestors(&[ID::new(999, 0)], &mut noop)
        .unwrap_err();
    assert!(matches!(
        err,
        ChangeTravelError::TargetIdNotFound(id) if id == ID::new(999, 0)
    ));

    let shallow = doc.export(ExportMode::shallow_snapshot(&v2))?;
    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&shallow)?;
    let mut noop = |_| ControlFlow::Continue(());
    let err = shallow_doc
        .travel_change_ancestors(&[v1_id], &mut noop)
        .unwrap_err();
    assert!(matches!(err, ChangeTravelError::TargetVersionNotIncluded));

    Ok(())
}

#[test]
fn tree_diff_apply_roundtrips_moves_creates_deletes_and_meta() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(66)?;

    let tree = doc.get_tree("outline");
    tree.enable_fractional_index(0);

    let root = tree.create(TreeParentId::Root)?;
    let child_a = tree.create_at(root, 0)?;
    let child_b = tree.create_at(root, 1)?;
    let child_c = tree.create_at(root, 2)?;
    tree.get_meta(root)?.insert("title", "v1-root")?;
    let details = tree
        .get_meta(root)?
        .insert_container("details", LoroMap::new())?;
    details.insert("owner", "alice")?;
    tree.get_meta(child_a)?.insert("title", "v1-a")?;
    tree.get_meta(child_b)?.insert("title", "v1-b")?;
    tree.get_meta(child_c)?.insert("title", "v1-c")?;
    doc.commit();

    let v1 = doc.state_frontiers();
    let snapshot_v1 = doc.export(ExportMode::Snapshot)?;

    tree.mov_before(child_c, child_a)?;
    let inserted = tree.create_at(root, 1)?;
    tree.get_meta(inserted)?.insert("title", "v2-inserted")?;
    tree.delete(child_b)?;
    tree.get_meta(root)?.insert("title", "v2-root")?;
    details.insert("status", "ready")?;
    doc.commit();

    let v2 = doc.state_frontiers();
    let snapshot_v2 = doc.export(ExportMode::Snapshot)?;
    assert_eq!(
        doc.get_tree("outline").children(root),
        Some(vec![child_c, inserted, child_a])
    );
    assert_eq!(
        doc.get_tree("outline")
            .get_meta(root)?
            .get("title")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("v2-root")
    );

    let forward = doc.diff(&v1, &v2)?;
    let patched = LoroDoc::from_snapshot(&snapshot_v1)?;
    patched.apply_diff(forward)?;
    let patched_tree = patched.get_tree("outline");
    let patched_children = patched_tree.children(root).unwrap();
    assert_eq!(patched_children.len(), 3);
    assert_eq!(
        patched_children
            .iter()
            .map(|id| {
                patched_tree
                    .get_meta(*id)
                    .unwrap()
                    .get("title")
                    .unwrap()
                    .get_deep_value()
                    .to_json_value()
            })
            .collect::<Vec<_>>(),
        vec![json!("v1-c"), json!("v2-inserted"), json!("v1-a")]
    );
    assert_eq!(
        patched_tree
            .get_meta(root)?
            .get("title")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("v2-root")
    );
    assert_eq!(
        patched_tree
            .get_meta(root)?
            .get("details")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!({"owner": "alice", "status": "ready"})
    );

    let reverse = doc.diff(&v2, &v1)?;
    let restored = LoroDoc::from_snapshot(&snapshot_v2)?;
    restored.apply_diff(reverse)?;
    let restored_tree = restored.get_tree("outline");
    let restored_children = restored_tree.children(root).unwrap();
    assert_eq!(restored_children.len(), 3);
    assert_eq!(
        restored_children
            .iter()
            .map(|id| {
                restored_tree
                    .get_meta(*id)
                    .unwrap()
                    .get("title")
                    .unwrap()
                    .get_deep_value()
                    .to_json_value()
            })
            .collect::<Vec<_>>(),
        vec![json!("v1-a"), json!("v1-b"), json!("v1-c")]
    );
    assert_eq!(
        restored_tree
            .get_meta(root)?
            .get("title")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("v1-root")
    );
    assert_eq!(
        restored_tree
            .get_meta(root)?
            .get("details")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!({"owner": "alice"})
    );

    Ok(())
}

#[test]
fn tree_diff_applies_move_into_deleted_parent_and_preserves_deleted_subtree_meta() -> LoroResult<()>
{
    let base = LoroDoc::new();
    base.set_peer_id(68)?;

    let tree = base.get_tree("outline");
    tree.enable_fractional_index(0);

    let root = tree.create(TreeParentId::Root)?;
    let live = tree.create_at(root, 0)?;
    let deleted_parent = tree.create_at(root, 1)?;
    let moved = tree.create_at(root, 2)?;

    tree.get_meta(root)?.insert("title", "base-root")?;
    tree.get_meta(live)?.insert("title", "live")?;
    tree.get_meta(deleted_parent)?.insert("title", "parent")?;
    tree.get_meta(moved)?.insert("title", "moved")?;
    base.commit();

    let base_frontiers = base.state_frontiers();
    let base_vv = base.oplog_vv();
    let base_snapshot = base.export(ExportMode::Snapshot)?;
    let base_updates = base.export(ExportMode::all_updates())?;

    let alice = LoroDoc::new();
    alice.set_peer_id(69)?;
    alice.import(&base_updates)?;
    let alice_tree = alice.get_tree("outline");
    alice_tree.delete(deleted_parent)?;
    alice.commit();
    let alice_updates = alice.export(ExportMode::updates(&base_vv))?;

    let bob = LoroDoc::new();
    bob.set_peer_id(70)?;
    bob.import(&base_updates)?;
    let bob_tree = bob.get_tree("outline");
    bob_tree.mov_to(moved, deleted_parent, 0)?;
    bob_tree.get_meta(moved)?.insert("branch", "bob")?;
    bob.commit();
    let bob_updates = bob.export(ExportMode::updates(&base_vv))?;

    let merged = LoroDoc::from_snapshot(&base_snapshot)?;
    merged.import(&alice_updates)?;
    merged.import(&bob_updates)?;

    let merged_tree = merged.get_tree("outline");
    assert_eq!(merged_tree.parent(root), Some(TreeParentId::Root));
    assert_eq!(merged_tree.parent(live), Some(TreeParentId::Node(root)));
    assert_eq!(
        merged_tree.parent(deleted_parent),
        Some(TreeParentId::Deleted)
    );
    assert_eq!(
        merged_tree.parent(moved),
        Some(TreeParentId::Node(deleted_parent))
    );
    assert!(merged_tree.is_node_deleted(&deleted_parent)?);
    assert!(merged_tree.is_node_deleted(&moved)?);
    assert_eq!(merged_tree.children(root), Some(vec![live]));
    assert_eq!(merged_tree.children(deleted_parent), Some(vec![moved]));
    assert_eq!(
        merged_tree
            .get_meta(deleted_parent)?
            .get("title")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("parent")
    );
    assert_eq!(
        merged_tree
            .get_meta(moved)?
            .get("branch")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("bob")
    );
    assert!(merged_tree
        .get_nodes(true)
        .iter()
        .any(|n| n.id == deleted_parent));
    assert!(merged_tree.get_nodes(true).iter().any(|n| n.id == moved));
    assert_eq!(
        merged_tree.get_nodes(false).iter().all(|n| n.id != moved),
        true
    );

    let merged_frontiers = merged.state_frontiers();
    let forward = merged.diff(&base_frontiers, &merged_frontiers)?;
    let patched = LoroDoc::from_snapshot(&base_snapshot)?;
    patched.apply_diff(forward)?;
    assert_eq!(deep_json(&patched), deep_json(&merged));
    assert!(patched.get_tree("outline").is_node_deleted(&moved)?);
    assert!(patched
        .get_tree("outline")
        .get_nodes(false)
        .iter()
        .all(|n| n.id != moved));

    Ok(())
}
