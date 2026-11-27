use loro::{ExportMode, LoroDoc, LoroResult, TreeID, TreeParentId, UndoManager};

fn sync(from: &LoroDoc, to: &LoroDoc) {
    let updates = from.export(ExportMode::updates(&to.oplog_vv())).unwrap();
    to.import(&updates).unwrap();
}

fn parent(doc: &LoroDoc, id: TreeID) -> TreeParentId {
    doc.get_tree("tree").parent(id).unwrap()
}

/// Undoing a move whose old parent was concurrently moved under the node would
/// form a cycle. That step can never succeed, so it must not block older
/// history (loro-dev/loro#1359).
#[test]
fn undo_move_back_under_a_descendant_is_skipped() -> LoroResult<()> {
    let a = LoroDoc::new();
    a.set_peer_id(2)?;
    let b = LoroDoc::new();
    b.set_peer_id(1)?;
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    let ta = a.get_tree("tree");
    let tb = b.get_tree("tree");

    let x = ta.create(None)?;
    let p1 = ta.create(None)?;
    let p2 = ta.create(None)?;
    a.commit();
    sync(&a, &b);
    ta.mov(x, p1)?;
    a.commit();
    ta.mov(x, p2)?;
    a.commit();
    // Ordered before a's first move, which is therefore ignored.
    tb.mov(p1, x)?;
    b.commit();
    sync(&b, &a);
    assert_eq!(parent(&a, x), TreeParentId::Node(p2));
    assert_eq!(parent(&a, p1), TreeParentId::Node(x));

    // The move under p2 cannot be reverted to p1, so x stays where it is.
    undo.undo()?;
    assert_eq!(parent(&a, p1), TreeParentId::Node(x));
    while undo.can_undo() {
        undo.undo()?;
    }
    // Older history is still reachable: a's creations are undone.
    assert!(!ta.contains(x) || ta.is_node_deleted(&x)?);
    assert!(!ta.contains(p2) || ta.is_node_deleted(&p2)?);

    while undo.can_redo() {
        undo.redo()?;
    }
    sync(&a, &b);
    assert_eq!(a.get_deep_value(), b.get_deep_value());
    Ok(())
}
