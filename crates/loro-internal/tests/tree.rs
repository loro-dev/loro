use loro_internal::{LoroDoc, TreeParentId};

#[test]
fn tree_index() {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(0).unwrap();
    let tree = doc.get_tree("tree");
    let root = tree.create(TreeParentId::Root).unwrap();
    let child = tree.create(root.into()).unwrap();
    let child2 = tree.create_at(root.into(), 0).unwrap();
    // sort with OpID
    assert_eq!(tree.get_index_by_tree_id(&child).unwrap(), 0);
    assert_eq!(tree.get_index_by_tree_id(&child2).unwrap(), 1);

    let doc = LoroDoc::new_auto_commit();
    doc.set_with_fractional_index(true);
    doc.set_peer_id(0).unwrap();
    let tree = doc.get_tree("tree");
    let root = tree.create(TreeParentId::Root).unwrap();
    let child = tree.create(root.into()).unwrap();
    let child2 = tree.create_at(root.into(), 0).unwrap();
    // sort with fractional index
    assert_eq!(tree.get_index_by_tree_id(&child).unwrap(), 1);
    assert_eq!(tree.get_index_by_tree_id(&child2).unwrap(), 0);
}
