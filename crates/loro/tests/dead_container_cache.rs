//! A container's "deleted" answer must follow every way the container can be
//! revived. `DocState` caches deletions (`dead_containers_cache.rs`); these
//! tests query `is_deleted()` first so that an entry exists, then revive the
//! container and check both the answer and that edits are accepted.
//!
//! On `main` the cache kept the stale answer in release builds (debug builds
//! recompute and only cross-check one entry), so run these with `--release`
//! as well.

use loro::{
    ContainerTrait, ExportMode, Frontiers, LoroDoc, LoroMap, LoroMovableList, LoroTree, TreeID,
    TreeParentId, UndoManager,
};

struct DeletedSubtree {
    doc: LoroDoc,
    tree: LoroTree,
    child: TreeID,
    grandchild: TreeID,
    /// The metadata of `grandchild`, queried while its grandparent is deleted.
    meta: LoroMap,
    alive_version: Frontiers,
}

/// `parent -> child -> grandchild`, with `parent` deleted and the deletion of
/// `grandchild`'s metadata already observed.
fn deleted_subtree(peer: u64) -> DeletedSubtree {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer).unwrap();
    let tree = doc.get_tree("tree");
    let parent = tree.create(TreeParentId::Root).unwrap();
    let child = tree.create(parent).unwrap();
    let grandchild = tree.create(child).unwrap();
    tree.get_meta(grandchild).unwrap().insert("v", 0).unwrap();
    doc.commit();
    let alive_version = doc.state_frontiers();
    tree.delete(parent).unwrap();
    doc.commit();

    let meta = tree.get_meta(grandchild).unwrap();
    assert!(meta.is_deleted());
    assert!(meta.insert("k", 1).is_err());
    DeletedSubtree {
        doc,
        tree,
        child,
        grandchild,
        meta,
        alive_version,
    }
}

fn assert_meta_alive_and_editable(s: &DeletedSubtree) {
    assert!(!s.tree.is_node_deleted(&s.grandchild).unwrap());
    assert!(!s.meta.is_deleted());
    s.meta.insert("after", 1).unwrap();
    s.doc.commit();
    assert_eq!(
        s.tree
            .get_meta(s.grandchild)
            .unwrap()
            .get("after")
            .unwrap()
            .into_value()
            .unwrap(),
        1.into()
    );
}

#[test]
fn local_move_revives_metadata_after_is_deleted_query() {
    let s = deleted_subtree(1);
    s.tree.mov(s.child, TreeParentId::Root).unwrap();
    s.doc.commit();
    assert_meta_alive_and_editable(&s);
}

#[test]
fn move_and_edit_in_one_transaction_after_is_deleted_query() {
    let s = deleted_subtree(1);
    s.tree.mov(s.child, TreeParentId::Root).unwrap();
    s.meta.insert("k", 1).unwrap();
    s.doc.commit();
    assert_meta_alive_and_editable(&s);
}

#[test]
fn imported_move_revives_metadata_after_is_deleted_query() {
    let s = deleted_subtree(1);
    let other = LoroDoc::new();
    other.set_peer_id(2).unwrap();
    other
        .import(&s.doc.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    other
        .get_tree("tree")
        .mov(s.child, TreeParentId::Root)
        .unwrap();
    other.commit();

    s.doc
        .import(
            &other
                .export(ExportMode::updates(&s.doc.oplog_vv()))
                .unwrap(),
        )
        .unwrap();
    assert_meta_alive_and_editable(&s);
}

#[test]
fn concurrent_move_revives_metadata_after_is_deleted_query() {
    // The delete and the move are concurrent; the move has the greater lamport,
    // so importing it revives the subtree.
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let tree = doc.get_tree("tree");
    let parent = tree.create(TreeParentId::Root).unwrap();
    let child = tree.create(parent).unwrap();
    doc.commit();
    let other = LoroDoc::new();
    other.set_peer_id(2).unwrap();
    other
        .import(&doc.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    tree.delete(parent).unwrap();
    doc.commit();
    let meta = tree.get_meta(child).unwrap();
    assert!(meta.is_deleted());

    let other_tree = other.get_tree("tree");
    other_tree.create(TreeParentId::Root).unwrap();
    other.commit();
    other_tree.mov(parent, TreeParentId::Root).unwrap();
    other.commit();
    doc.import(&other.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    assert!(!tree.is_node_deleted(&child).unwrap());
    assert!(!meta.is_deleted());
    meta.insert("k", 1).unwrap();
    doc.commit();
}

/// `node` moved under a deleted node, with the deletion of its metadata
/// already observed.
fn node_moved_into_deleted_subtree(doc: &LoroDoc) -> (LoroTree, TreeID, LoroMap, Frontiers) {
    let tree = doc.get_tree("tree");
    let deleted = tree.create(TreeParentId::Root).unwrap();
    let node = tree.create(TreeParentId::Root).unwrap();
    tree.get_meta(node).unwrap().insert("v", 1).unwrap();
    doc.commit();
    tree.delete(deleted).unwrap();
    doc.commit();
    let alive_version = doc.state_frontiers();
    tree.mov(node, deleted).unwrap();
    doc.commit();

    let meta = tree.get_meta(node).unwrap();
    assert!(meta.is_deleted());
    assert!(meta.insert("k", 1).is_err());
    (tree, node, meta, alive_version)
}

/// Undo and `revert_to` go through `Handler::apply_diff`, which never moves a
/// deleted node back: it creates a new node and copies the metadata. The old
/// metadata stays deleted, and the new one is alive.
fn assert_recreated_as_new_node(tree: &LoroTree, node: TreeID, meta: &LoroMap) {
    assert!(tree.is_node_deleted(&node).unwrap());
    assert!(meta.is_deleted());
    assert!(meta.insert("k", 1).is_err());

    let roots = tree.roots();
    assert_eq!(roots.len(), 1);
    assert_ne!(roots[0], node);
    let new_meta = tree.get_meta(roots[0]).unwrap();
    assert!(!new_meta.is_deleted());
    assert_eq!(new_meta.get("v").unwrap().into_value().unwrap(), 1.into());
    new_meta.insert("k", 1).unwrap();
}

#[test]
fn undo_after_is_deleted_query_recreates_node() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let mut undo = UndoManager::new(&doc);
    let (tree, node, meta, _) = node_moved_into_deleted_subtree(&doc);

    assert!(undo.undo().unwrap());
    assert_recreated_as_new_node(&tree, node, &meta);
    doc.commit();
}

#[test]
fn revert_to_after_is_deleted_query_recreates_node() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let (tree, node, meta, alive_version) = node_moved_into_deleted_subtree(&doc);

    doc.revert_to(&alive_version).unwrap();
    doc.commit();
    assert_recreated_as_new_node(&tree, node, &meta);
    doc.commit();
}

#[test]
fn checkout_revives_metadata_after_is_deleted_query() {
    let s = deleted_subtree(1);
    s.doc.checkout(&s.alive_version).unwrap();
    assert!(!s.tree.is_node_deleted(&s.grandchild).unwrap());
    assert!(!s.meta.is_deleted());

    s.doc.checkout_to_latest();
    assert!(s.meta.is_deleted());
}

#[test]
fn imported_movable_list_move_revives_child_after_is_deleted_query() {
    // Loro keeps a movable-list element deleted by one peer if another peer
    // concurrently moves it with a greater lamport.
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let list = a.get_movable_list("list");
    list.insert(0, 0).unwrap();
    let child = list.insert_container(1, LoroMap::new()).unwrap();
    child.insert("x", 1).unwrap();
    a.commit();

    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    b.get_map("pad").insert("k", 1).unwrap();
    b.commit();
    b.get_movable_list("list").mov(1, 0).unwrap();
    b.commit();

    list.delete(1, 1).unwrap();
    a.commit();
    assert!(child.is_deleted());
    assert!(child.insert("y", 1).is_err());

    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(list.len(), 2);
    assert!(!child.is_deleted());
    child.insert("y", 2).unwrap();
    a.commit();
}

#[test]
fn map_and_list_deletions_stay_deleted() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let map = doc.get_map("map");
    let in_map = map.insert_container("k", LoroMap::new()).unwrap();
    let list = doc.get_list("list");
    let in_list = list.insert_container(0, LoroMovableList::new()).unwrap();
    doc.commit();
    map.delete("k").unwrap();
    list.delete(0, 1).unwrap();
    doc.commit();

    for _ in 0..2 {
        assert!(in_map.is_deleted());
        assert!(in_list.is_deleted());
        assert!(in_map.insert("x", 1).is_err());
        assert!(in_list.insert(0, 1).is_err());
    }
}
