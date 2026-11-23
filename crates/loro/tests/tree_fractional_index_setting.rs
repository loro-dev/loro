use loro::{ExportMode, LoroDoc, TreeParentId};

/// Appends a root node and returns the byte length of its fractional index:
/// 6 with jitter 4, 2 without.
fn append_len(doc: &LoroDoc) -> usize {
    let tree = doc.get_tree("tree");
    let node = tree.create(TreeParentId::Root).unwrap();
    tree.fractional_index(node).unwrap().len() / 2
}

fn source() -> LoroDoc {
    let doc = LoroDoc::new();
    doc.get_tree("tree").create(TreeParentId::Root).unwrap();
    doc.commit();
    doc
}

/// loro-dev/loro#1288: a tree's jitter setting must survive importing a
/// snapshot into an empty doc, as it does for an update import.
#[test]
fn tree_jitter_survives_import() {
    let src = source();
    for mode in [ExportMode::all_updates(), ExportMode::Snapshot] {
        let doc = LoroDoc::new();
        doc.get_tree("tree").enable_fractional_index(4);
        doc.import(&src.export(mode.clone()).unwrap()).unwrap();
        assert_eq!(append_len(&doc), 6, "{mode:?}");
    }
}

#[test]
fn tree_jitter_survives_shallow_snapshot_import() {
    let src = source();
    src.get_tree("tree").create(TreeParentId::Root).unwrap();
    src.commit();
    let bytes = src
        .export(ExportMode::shallow_snapshot(&src.oplog_frontiers()))
        .unwrap();
    let doc = LoroDoc::new();
    doc.get_tree("tree").enable_fractional_index(4);
    doc.import(&bytes).unwrap();
    assert_eq!(append_len(&doc), 6);
}

#[test]
fn disabled_fractional_index_survives_snapshot_import() {
    let src = source();
    let doc = LoroDoc::new();
    let tree = doc.get_tree("tree");
    tree.disable_fractional_index();
    doc.import(&src.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert!(!doc.get_tree("tree").is_fractional_index_enabled());
}

#[test]
fn tree_jitter_survives_fork() {
    let doc = LoroDoc::new();
    doc.get_tree("tree").enable_fractional_index(4);
    doc.get_tree("tree").create(TreeParentId::Root).unwrap();
    doc.commit();
    assert_eq!(append_len(&doc), 6);
    doc.commit();

    assert_eq!(append_len(&doc.fork()), 6);
    assert_eq!(append_len(&doc.fork_at(&doc.oplog_frontiers()).unwrap()), 6);

    // A detached doc forks through `fork_at`.
    doc.checkout(&doc.oplog_frontiers()).unwrap();
    assert_eq!(append_len(&doc.fork()), 6);
}

#[test]
fn disabled_fractional_index_survives_fork() {
    let doc = LoroDoc::new();
    doc.get_tree("tree").disable_fractional_index();
    doc.get_tree("tree").create(TreeParentId::Root).unwrap();
    doc.commit();
    assert!(!doc.fork().get_tree("tree").is_fractional_index_enabled());
}
