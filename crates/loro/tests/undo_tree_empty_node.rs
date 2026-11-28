//! Undo of a tree move must not leave a live node whose metadata map is empty
//! when a remote edit deleted that node or its new parent (loro-dev/loro#1311).
//!
//! The literal two-peer repro in `undo_tree_node_identity.rs` passes. What
//! remains is a move whose recorded inverse is only a `Move`: applying it
//! revives the deleted node, and the missing map entry used to be treated as
//! an empty full state.

use loro::{ExportMode, LoroDoc, LoroValue, TreeID, TreeParentId, UndoManager};

fn sync(from: &LoroDoc, to: &LoroDoc) {
    to.import(&from.export(ExportMode::updates(&to.oplog_vv())).unwrap())
        .unwrap();
}

fn peer(id: u64) -> (LoroDoc, UndoManager) {
    let doc = LoroDoc::new();
    doc.set_peer_id(id).unwrap();
    let mut undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    (doc, undo)
}

fn named(tree: &loro::LoroTree, parent: impl Into<TreeParentId>, name: &str) -> TreeID {
    let id = tree.create(parent).unwrap();
    tree.get_meta(id).unwrap().insert("name", name).unwrap();
    id
}

fn name_of(doc: &LoroDoc, id: TreeID) -> Option<String> {
    let meta = doc.get_tree("t").get_meta(id).ok()?;
    match meta.get("name")?.into_value().ok()? {
        LoroValue::String(s) => Some(s.to_string()),
        _ => None,
    }
}

fn empty_live(doc: &LoroDoc) -> Vec<TreeID> {
    let tree = doc.get_tree("t");
    tree.get_nodes(false)
        .into_iter()
        .filter(|node| {
            tree.get_meta(node.id)
                .ok()
                .is_none_or(|meta| meta.is_empty())
        })
        .map(|node| node.id)
        .collect()
}

/// B moves C from P to Q. After that is synced, A deletes C. B undoes the move.
#[test]
fn undo_move_after_remote_delete_of_moved_node() {
    let (a, _ua) = peer(1);
    let (b, mut ub) = peer(2);
    let ta = a.get_tree("t");
    let p = named(&ta, TreeParentId::Root, "P");
    let c = named(&ta, p, "C");
    let q = named(&ta, TreeParentId::Root, "Q");
    a.commit();
    sync(&a, &b);

    let tb = b.get_tree("t");
    tb.mov(c, q).unwrap();
    b.commit();
    sync(&b, &a);
    ta.delete(c).unwrap();
    a.commit();
    sync(&a, &b);
    assert!(tb.is_node_deleted(&c).unwrap());

    assert!(ub.undo().unwrap());
    assert!(!tb.is_node_deleted(&c).unwrap());
    assert_eq!(tb.children(p).unwrap(), vec![c]);
    assert_eq!(name_of(&b, c).as_deref(), Some("C"));
    assert!(empty_live(&b).is_empty(), "live node with empty data");
}

/// B moves C from P to Q. After that is synced, A deletes Q. B undoes the move.
#[test]
fn undo_move_after_remote_delete_of_new_parent() {
    let (a, _ua) = peer(1);
    let (b, mut ub) = peer(2);
    let ta = a.get_tree("t");
    let p = named(&ta, TreeParentId::Root, "P");
    let c = named(&ta, p, "C");
    let q = named(&ta, TreeParentId::Root, "Q");
    a.commit();
    sync(&a, &b);

    let tb = b.get_tree("t");
    tb.mov(c, q).unwrap();
    b.commit();
    sync(&b, &a);
    ta.delete(q).unwrap();
    a.commit();
    sync(&a, &b);
    assert!(tb.is_node_deleted(&c).unwrap());
    assert!(tb.is_node_deleted(&q).unwrap());

    assert!(ub.undo().unwrap());
    assert!(!tb.is_node_deleted(&c).unwrap());
    assert!(tb.is_node_deleted(&q).unwrap());
    assert_eq!(tb.children(p).unwrap(), vec![c]);
    assert_eq!(name_of(&b, c).as_deref(), Some("C"));
    assert!(empty_live(&b).is_empty(), "live node with empty data");
}
