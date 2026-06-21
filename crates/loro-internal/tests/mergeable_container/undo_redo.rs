//! Undo/redo coverage for mergeable child markers.
//!
//! Map diffs expose active mergeable markers as `Container` values at read/event
//! boundaries. Applying those diffs must write the marker for the destination
//! parent/key instead of recreating the child as a regular container.

#[path = "common.rs"]
mod common;
use common::doc;

use loro_common::{ContainerID, LoroValue};
use loro_internal::{handler::MapHandler, HandlerTrait, TreeParentId, UndoManager};

fn expect_container(value: Option<LoroValue>, label: &str) -> ContainerID {
    match value {
        Some(LoroValue::Container(id)) => id,
        other => panic!("expected {label} to be a container value, got {other:?}"),
    }
}

#[test]
fn undo_redo_restores_mergeable_child_marker_on_map() {
    let doc = doc(1);
    let root = doc.get_map("state");
    let undo = UndoManager::new(&doc);

    let attrs = root.ensure_mergeable_map("attrs").unwrap();
    attrs.insert("type", "paragraph").unwrap();
    doc.commit_then_renew();
    let attrs_id = attrs.id();
    assert!(attrs_id.is_mergeable());

    assert!(undo.undo().unwrap());
    assert_eq!(root.get("attrs"), None);

    assert!(undo.redo().unwrap());
    let restored_id = expect_container(root.get("attrs"), "restored attrs");
    assert_eq!(
        restored_id, attrs_id,
        "same parent/key should restore the same deterministic mergeable cid"
    );

    let restored_attrs = root.ensure_mergeable_map("attrs").unwrap();
    assert_eq!(restored_attrs.id(), attrs_id);
    assert_eq!(restored_attrs.get("type"), Some("paragraph".into()));
}

#[test]
fn undo_delete_restores_mergeable_child_marker_on_existing_map() {
    let doc = doc(1);
    let root = doc.get_map("state");
    let attrs = root.ensure_mergeable_map("attrs").unwrap();
    attrs.insert("type", "paragraph").unwrap();
    doc.commit_then_renew();
    let attrs_id = attrs.id();

    let undo = UndoManager::new(&doc);
    root.delete("attrs").unwrap();
    doc.commit_then_renew();
    assert_eq!(root.get("attrs"), None);

    assert!(undo.undo().unwrap());
    let restored_id = expect_container(root.get("attrs"), "restored attrs");
    assert_eq!(
        restored_id, attrs_id,
        "undoing a mergeable child delete should restore the marker for the same cid"
    );

    let restored_attrs = root.ensure_mergeable_map("attrs").unwrap();
    assert_eq!(restored_attrs.id(), attrs_id);
    assert_eq!(restored_attrs.get("type"), Some("paragraph".into()));
}

#[test]
fn undo_parent_map_delete_restores_nested_mergeable_marker() {
    let doc = doc(1);
    let root = doc.get_map("state");
    let parent = root
        .insert_container("parent", MapHandler::new_detached())
        .unwrap();
    let attrs = parent.ensure_mergeable_map("attrs").unwrap();
    attrs.insert("type", "paragraph").unwrap();
    doc.commit_then_renew();

    let undo = UndoManager::new(&doc);
    root.delete("parent").unwrap();
    doc.commit_then_renew();
    assert_eq!(root.get("parent"), None);

    assert!(undo.undo().unwrap());
    let restored_parent = root
        .get_child_handler("parent")
        .unwrap()
        .into_map()
        .unwrap();
    let restored_attrs_id = expect_container(restored_parent.get("attrs"), "restored attrs");
    assert!(
        restored_attrs_id.is_mergeable(),
        "nested mergeable child must be restored as a marker, not a regular child cid"
    );

    let restored_attrs = restored_parent.ensure_mergeable_map("attrs").unwrap();
    assert_eq!(restored_attrs.id(), restored_attrs_id);
    assert_eq!(restored_attrs.get("type"), Some("paragraph".into()));
}

#[test]
fn undo_mergeable_parent_map_delete_resurfaces_nested_mergeable_child() {
    let doc = doc(1);
    let root = doc.get_map("state");
    let parent = root.ensure_mergeable_map("parent").unwrap();
    let attrs = parent.ensure_mergeable_map("attrs").unwrap();
    attrs.insert("type", "paragraph").unwrap();
    doc.commit_then_renew();
    let parent_id = parent.id();
    let attrs_id = attrs.id();

    let undo = UndoManager::new(&doc);
    root.delete("parent").unwrap();
    doc.commit_then_renew();
    assert_eq!(root.get("parent"), None);

    assert!(undo.undo().unwrap());
    let restored_parent_id = expect_container(root.get("parent"), "restored parent");
    assert_eq!(restored_parent_id, parent_id);

    let restored_parent = root.ensure_mergeable_map("parent").unwrap();
    let restored_attrs_id = expect_container(restored_parent.get("attrs"), "restored attrs");
    assert_eq!(
        restored_attrs_id, attrs_id,
        "reactivating a mergeable parent should resurface its nested mergeable child"
    );
    let restored_attrs = restored_parent.ensure_mergeable_map("attrs").unwrap();
    assert_eq!(restored_attrs.get("type"), Some("paragraph".into()));
}

#[test]
fn redo_restores_tree_node_mergeable_meta_child_as_marker() {
    let doc = doc(1);
    let tree = doc.get_tree("root");
    let root = tree.create(TreeParentId::Root).unwrap();
    tree.get_meta(root).unwrap().insert("name", "root").unwrap();

    let target = tree.create(TreeParentId::Node(root)).unwrap();
    tree.get_meta(target)
        .unwrap()
        .insert("name", "target")
        .unwrap();
    doc.commit_then_renew();

    let undo = UndoManager::new(&doc);
    let child = tree.create(TreeParentId::Root).unwrap();
    let child_meta = tree.get_meta(child).unwrap();
    child_meta.insert("name", "child").unwrap();
    let attrs = child_meta.ensure_mergeable_map("sys_attrs").unwrap();
    attrs.insert("type", "paragraph").unwrap();
    tree.mov(child, TreeParentId::Node(target)).unwrap();
    doc.commit_then_renew();

    assert!(undo.undo().unwrap());
    assert!(undo.redo().unwrap());

    let target_children = tree.children(&TreeParentId::Node(target)).unwrap();
    let restored_child = target_children
        .into_iter()
        .find(|node| tree.get_meta(*node).unwrap().get("name") == Some("child".into()))
        .expect("redo should restore the child under target");
    let restored_meta = tree.get_meta(restored_child).unwrap();
    let restored_attrs_id = expect_container(restored_meta.get("sys_attrs"), "sys_attrs");
    assert!(
        restored_attrs_id.is_mergeable(),
        "tree meta child should remain a mergeable cid after redo"
    );

    let restored_attrs = restored_meta.ensure_mergeable_map("sys_attrs").unwrap();
    assert_eq!(restored_attrs.id(), restored_attrs_id);
    assert_eq!(restored_attrs.get("type"), Some("paragraph".into()));
}
