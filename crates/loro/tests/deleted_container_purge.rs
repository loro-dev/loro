use loro::{
    ContainerID, ContainerTrait, ContainerType, ExportMode, LoroDoc, LoroError, LoroText,
    TreeParentId,
};
use serial_test::parallel;
use std::sync::{Arc, Barrier};

const SECRET_TEXT: &str = "PURGE-PROBE-TEXT-c0ffee";
const SECRET_MARK: &str = "PURGE-PROBE-MARK-deadbeef";

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

fn shallow(doc: &LoroDoc) -> Vec<u8> {
    doc.export(ExportMode::shallow_snapshot(&doc.oplog_frontiers()))
        .unwrap()
}

fn sync(a: &LoroDoc, b: &LoroDoc) {
    a.import(&b.export(ExportMode::updates(&a.oplog_vv())).unwrap())
        .unwrap();
    b.import(&a.export(ExportMode::updates(&b.oplog_vv())).unwrap())
        .unwrap();
}

/// The returned mergeable text holds both secrets; its owning tree node is deleted.
fn doc_with_deleted_owner(peer: u64) -> (LoroDoc, ContainerID) {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer).unwrap();

    let tree = doc.get_tree("blocks");
    let node = tree.create(TreeParentId::Root).unwrap();
    let meta = tree.get_meta(node).unwrap();
    let text = meta.ensure_mergeable_text("body").unwrap();
    let cid = text.id();

    text.insert(0, SECRET_TEXT).unwrap();
    text.mark(0..SECRET_TEXT.len(), "comment", SECRET_MARK)
        .unwrap();
    doc.commit();

    tree.delete(node).unwrap();
    doc.commit();

    (doc, cid)
}

#[test]
#[parallel]
fn purge_of_deleted_owner_container_clears_shallow_snapshot() {
    let (doc, cid) = doc_with_deleted_owner(1);

    assert!(
        contains(&shallow(&doc), SECRET_TEXT),
        "precondition: the unreachable content is in the shallow snapshot"
    );

    doc.delete_root_container(cid).unwrap();
    doc.commit();

    let after = shallow(&doc);
    assert!(
        !contains(&after, SECRET_TEXT),
        "text secret survives shallow-snapshot export after purge"
    );
    assert!(
        !contains(&after, SECRET_MARK),
        "mark secret survives shallow-snapshot export after purge"
    );

    let fresh = LoroDoc::new();
    fresh.import(&after).unwrap();
    assert!(!contains(&shallow(&fresh), SECRET_TEXT));
    assert!(!contains(
        &fresh.export(ExportMode::Snapshot).unwrap(),
        SECRET_TEXT
    ));
}

#[test]
#[parallel]
fn purging_and_non_purging_peers_converge() {
    let (a, cid) = doc_with_deleted_owner(1);
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    b.import(&a.export(ExportMode::Snapshot).unwrap()).unwrap();

    a.delete_root_container(cid).unwrap();
    a.commit();
    sync(&a, &b);

    assert_eq!(
        a.get_deep_value(),
        b.get_deep_value(),
        "purging and non-purging peers diverged on document value"
    );
    assert!(!contains(&shallow(&a), SECRET_TEXT));
    assert!(
        !contains(&shallow(&b), SECRET_TEXT),
        "peer that never purged still leaks the secret through its own shallow snapshot"
    );
}

#[test]
#[parallel]
fn concurrent_purge_and_resurrect_converge() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let tree = a.get_tree("blocks");
    let node = tree.create(TreeParentId::Root).unwrap();
    let other_parent = tree.create(TreeParentId::Root).unwrap();
    let text = tree
        .get_meta(node)
        .unwrap()
        .ensure_mergeable_text("body")
        .unwrap();
    text.insert(0, SECRET_TEXT).unwrap();
    a.commit();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    b.import(&a.export(ExportMode::Snapshot).unwrap()).unwrap();

    tree.delete(node).unwrap();
    a.commit();
    a.delete_root_container(text.id()).unwrap();
    a.commit();

    // Raises B's lamport so its concurrent move wins over A's delete.
    for i in 0..5 {
        b.get_map("pad").insert("i", i).unwrap();
        b.commit();
    }
    let b_tree = b.get_tree("blocks");
    b_tree.mov(node, other_parent).unwrap();
    let b_text = b_tree
        .get_meta(node)
        .unwrap()
        .ensure_mergeable_text("body")
        .unwrap();
    assert_eq!(b_text.id(), text.id());
    b_text.insert(0, "REVIVED ").unwrap();
    b.commit();

    sync(&a, &b);

    assert_eq!(
        a.get_deep_value(),
        b.get_deep_value(),
        "concurrent purge and resurrect diverged"
    );
    assert!(
        a.get_tree("blocks").contains(node),
        "the move must revive the node"
    );
    assert_eq!(a.get_text(text.id()).to_string(), "REVIVED ");
}

#[test]
#[parallel]
fn deleted_container_rejects_writes_after_purge() {
    let (doc, cid) = doc_with_deleted_owner(1);
    doc.delete_root_container(cid.clone()).unwrap();

    let err = doc.get_text(cid).insert(0, "after").unwrap_err();
    assert!(matches!(err, LoroError::ContainerDeleted { .. }), "{err:?}");
}

#[test]
#[parallel]
fn purge_errors() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.get_text("present");

    // Plain roots are lazily created, so any name "exists"; a mergeable root does not.
    let absent_mergeable =
        ContainerID::new_mergeable(&doc.get_map("nothing").id(), "body", ContainerType::Text);
    let err = doc.delete_root_container(absent_mergeable).unwrap_err();
    assert!(matches!(err, LoroError::NotFoundError(..)), "{err:?}");

    let map = doc.get_map("m");
    let nested = map
        .insert_container("inner", loro::LoroText::new())
        .unwrap();
    let err = doc.delete_root_container(nested.id()).unwrap_err();
    assert!(matches!(err, LoroError::ArgErr(..)), "{err:?}");

    let present = ContainerID::new_root("present", ContainerType::Text);
    doc.get_text(present.clone()).insert(0, "x").unwrap();
    doc.commit();
    doc.detach();
    let err = doc.delete_root_container(present.clone()).unwrap_err();
    assert!(matches!(err, LoroError::AutoCommitNotStarted), "{err:?}");

    doc.attach();
    doc.delete_root_container(present).unwrap();
}

#[test]
#[parallel]
fn purge_of_live_root_container() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("body");
    text.insert(0, SECRET_TEXT).unwrap();
    doc.commit();

    doc.delete_root_container(text.id()).unwrap();
    doc.commit();

    assert!(!contains(&shallow(&doc), SECRET_TEXT));
}

/// The returned mergeable texts each hold content; their owning tree nodes are deleted.
fn doc_with_deleted_owners(n: usize) -> (LoroDoc, Vec<LoroText>) {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let tree = doc.get_tree("blocks");
    let mut nodes = Vec::new();
    let mut texts = Vec::new();
    for i in 0..n {
        let node = tree.create(TreeParentId::Root).unwrap();
        let text = tree
            .get_meta(node)
            .unwrap()
            .ensure_mergeable_text("body")
            .unwrap();
        text.insert(0, &format!("content-{i}")).unwrap();
        nodes.push(node);
        texts.push(text);
    }
    doc.commit();
    for node in nodes {
        tree.delete(node).unwrap();
    }
    doc.commit();
    (doc, texts)
}

#[test]
#[parallel]
fn concurrent_purges_of_different_containers_all_succeed() {
    for _ in 0..200 {
        let (doc, texts) = doc_with_deleted_owners(8);
        let barrier = Arc::new(Barrier::new(2));
        let workers: Vec<_> = [0, 1]
            .into_iter()
            .map(|parity| {
                let doc = doc.clone();
                let barrier = barrier.clone();
                let ids: Vec<ContainerID> = texts
                    .iter()
                    .skip(parity)
                    .step_by(2)
                    .map(|text| text.id())
                    .collect();
                std::thread::spawn(move || {
                    barrier.wait();
                    ids.into_iter()
                        .map(|id| doc.delete_root_container(id))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for worker in workers {
            for result in worker.join().unwrap() {
                result.unwrap();
            }
        }
    }
}

#[test]
#[parallel]
fn writes_to_a_deleted_container_are_rejected_during_its_purge() {
    for _ in 0..300 {
        let (doc, texts) = doc_with_deleted_owners(1);
        let text = texts[0].clone();
        let barrier = Arc::new(Barrier::new(2));
        let purge = {
            let doc = doc.clone();
            let barrier = barrier.clone();
            let id = text.id();
            std::thread::spawn(move || {
                barrier.wait();
                doc.delete_root_container(id)
            })
        };
        barrier.wait();
        for _ in 0..200 {
            let err = text.insert(0, "late").unwrap_err();
            assert!(matches!(err, LoroError::ContainerDeleted { .. }), "{err:?}");
        }
        purge.join().unwrap().unwrap();
        doc.commit();
        assert_eq!(text.to_string(), "");
    }
}

const SECRET_NESTED: &str = "PURGE-PROBE-NESTED-5eed";

/// Returns the mergeable root created by `make` under a tree node that is then deleted.
fn purge_under_deleted_owner<C>(make: impl FnOnce(&loro::LoroMap) -> C) -> (LoroDoc, C) {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let tree = doc.get_tree("blocks");
    let node = tree.create(TreeParentId::Root).unwrap();
    let root = make(&tree.get_meta(node).unwrap());
    doc.commit();
    tree.delete(node).unwrap();
    doc.commit();
    (doc, root)
}

fn assert_secret_present(doc: &LoroDoc) {
    assert!(
        contains(
            &doc.export(ExportMode::state_only(None)).unwrap(),
            SECRET_NESTED
        ),
        "precondition: the nested secret is in the state_only export"
    );
}

fn assert_secret_gone(doc: &LoroDoc) {
    let state_only = doc.export(ExportMode::state_only(None)).unwrap();
    assert!(
        !contains(&state_only, SECRET_NESTED),
        "nested secret survives state_only export after purge"
    );
    let fresh = LoroDoc::new();
    fresh.import(&state_only).unwrap();
    assert!(!contains(
        &fresh.export(ExportMode::state_only(None)).unwrap(),
        SECRET_NESTED
    ));
    assert!(!contains(&shallow(doc), SECRET_NESTED));
}

#[test]
#[parallel]
fn purge_empties_nested_child_of_mergeable_map() {
    let (doc, map) = purge_under_deleted_owner(|meta| {
        let map = meta.ensure_mergeable_map("mm").unwrap();
        let child = map.insert_container("child", LoroText::new()).unwrap();
        child.insert(0, SECRET_NESTED).unwrap();
        map
    });
    let child = map
        .get("child")
        .unwrap()
        .into_container()
        .unwrap()
        .into_text()
        .unwrap();

    assert_secret_present(&doc);
    doc.delete_root_container(map.id()).unwrap();
    doc.commit();

    assert_eq!(doc.get_text(child.id()).to_string(), "");
    assert_secret_gone(&doc);
    let err = child.insert(0, "after").unwrap_err();
    assert!(matches!(err, LoroError::ContainerDeleted { .. }), "{err:?}");
}

#[test]
#[parallel]
fn purge_empties_nested_child_of_mergeable_list() {
    let (doc, list) = purge_under_deleted_owner(|meta| {
        let list = meta.ensure_mergeable_list("ml").unwrap();
        let child = list.insert_container(0, LoroText::new()).unwrap();
        child.insert(0, SECRET_NESTED).unwrap();
        list
    });
    let child = list
        .get(0)
        .unwrap()
        .into_container()
        .unwrap()
        .into_text()
        .unwrap();

    assert_secret_present(&doc);
    doc.delete_root_container(list.id()).unwrap();
    doc.commit();

    assert_eq!(doc.get_text(child.id()).to_string(), "");
    assert_secret_gone(&doc);
    let err = child.insert(0, "after").unwrap_err();
    assert!(matches!(err, LoroError::ContainerDeleted { .. }), "{err:?}");
}

#[test]
#[parallel]
fn purge_empties_node_metadata_of_mergeable_tree() {
    let (doc, (tree, node)) = purge_under_deleted_owner(|meta| {
        let tree = meta.ensure_mergeable_tree("mt").unwrap();
        let node = tree.create(TreeParentId::Root).unwrap();
        tree.get_meta(node)
            .unwrap()
            .insert("k", SECRET_NESTED)
            .unwrap();
        (tree, node)
    });
    let node_meta = tree.get_meta(node).unwrap();

    assert_secret_present(&doc);
    doc.delete_root_container(tree.id()).unwrap();
    doc.commit();

    assert_eq!(doc.get_map(node_meta.id()).len(), 0);
    assert_secret_gone(&doc);
    let err = node_meta.insert("k", "after").unwrap_err();
    assert!(matches!(err, LoroError::ContainerDeleted { .. }), "{err:?}");
}

/// Asserts that `text` holds no secret in `doc`, and that neither a shallow snapshot nor a
/// `state_only` export of `doc` carries it, as bytes or after import into a fresh doc.
fn assert_text_secret_gone(doc: &LoroDoc, text: &ContainerID) {
    assert_eq!(doc.get_text(text.clone()).to_string(), "");
    for (name, bytes) in [
        ("shallow", shallow(doc)),
        (
            "state_only",
            doc.export(ExportMode::state_only(None)).unwrap(),
        ),
    ] {
        assert!(
            !contains(&bytes, SECRET_NESTED),
            "{name} export bytes carry the secret"
        );
        let fresh = LoroDoc::new();
        fresh.import(&bytes).unwrap();
        if fresh.has_container(text) {
            assert_eq!(
                fresh.get_text(text.clone()).to_string(),
                "",
                "{name} import carries the secret"
            );
        }
        assert!(!contains(
            &fresh.export(ExportMode::state_only(None)).unwrap(),
            SECRET_NESTED
        ));
    }
}

#[test]
#[parallel]
fn purge_of_live_tree_empties_mergeable_node_bodies() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let tree = doc.get_tree("blocks");
    let node = tree.create(TreeParentId::Root).unwrap();
    let body = tree
        .get_meta(node)
        .unwrap()
        .ensure_mergeable_text("body")
        .unwrap();
    body.insert(0, SECRET_NESTED).unwrap();
    doc.commit();
    assert_secret_present(&doc);

    doc.delete_root_container(tree.id()).unwrap();
    doc.commit();

    assert_text_secret_gone(&doc, &body.id());
    let err = body.insert(0, "after").unwrap_err();
    assert!(matches!(err, LoroError::ContainerDeleted { .. }), "{err:?}");
}

#[test]
#[parallel]
fn purge_empties_mergeable_child_of_mergeable_map() {
    let (doc, (map, inner)) = purge_under_deleted_owner(|meta| {
        let map = meta.ensure_mergeable_map("mm").unwrap();
        let inner = map.ensure_mergeable_text("inner").unwrap();
        inner.insert(0, SECRET_NESTED).unwrap();
        (map, inner)
    });
    assert_secret_present(&doc);

    doc.delete_root_container(map.id()).unwrap();
    doc.commit();

    assert_text_secret_gone(&doc, &inner.id());
    let err = inner.insert(0, "after").unwrap_err();
    assert!(matches!(err, LoroError::ContainerDeleted { .. }), "{err:?}");
}

/// Returns an owner-deleted mergeable map and a mergeable text in it that holds the secret.
/// `orphan` runs before the owner is deleted, so it can detach the text from the map.
fn purge_orphaned_mergeable(
    orphan: impl FnOnce(&LoroDoc, &loro::LoroMap),
) -> (LoroDoc, loro::LoroMap, LoroText) {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let tree = doc.get_tree("blocks");
    let node = tree.create(TreeParentId::Root).unwrap();
    let map = tree
        .get_meta(node)
        .unwrap()
        .ensure_mergeable_map("mm")
        .unwrap();
    let inner = map.ensure_mergeable_text("inner").unwrap();
    inner.insert(0, SECRET_NESTED).unwrap();
    doc.commit();
    orphan(&doc, &map);
    doc.commit();
    tree.delete(node).unwrap();
    doc.commit();
    assert_secret_present(&doc);
    (doc, map, inner)
}

fn assert_orphan_purged(doc: &LoroDoc, map: &loro::LoroMap, inner: &LoroText) {
    doc.delete_root_container(map.id()).unwrap();
    doc.commit();
    assert_text_secret_gone(doc, &inner.id());
    let err = inner.insert(0, "after").unwrap_err();
    assert!(matches!(err, LoroError::ContainerDeleted { .. }), "{err:?}");
}

#[test]
#[parallel]
fn purge_empties_mergeable_orphaned_by_local_overwrite() {
    let (doc, map, inner) = purge_orphaned_mergeable(|_, map| {
        map.insert("inner", 1).unwrap();
    });
    assert_orphan_purged(&doc, &map, &inner);
}

#[test]
#[parallel]
fn purge_empties_mergeable_orphaned_by_local_delete() {
    let (doc, map, inner) = purge_orphaned_mergeable(|_, map| {
        map.delete("inner").unwrap();
    });
    assert_orphan_purged(&doc, &map, &inner);
}

#[test]
#[parallel]
fn purge_empties_mergeable_orphaned_by_concurrent_overwrite() {
    let (doc, map, inner) = purge_orphaned_mergeable(|doc, map| {
        let other = LoroDoc::new();
        other.set_peer_id(2).unwrap();
        other
            .import(&doc.export(ExportMode::Snapshot).unwrap())
            .unwrap();
        doc.get_map("local").insert("k", 1).unwrap();
        doc.commit();
        other.get_map("bump").insert("k", 1).unwrap();
        other.get_map(map.id()).insert("inner", 2).unwrap();
        other.commit();
        sync(doc, &other);
    });
    assert_orphan_purged(&doc, &map, &inner);
}
