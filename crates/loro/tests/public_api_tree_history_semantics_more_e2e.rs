use loro::{
    EncodedBlobMode, ExportMode, LoroDoc, LoroError, LoroList, LoroMap, LoroResult, ToJson,
    TreeParentId,
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

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
