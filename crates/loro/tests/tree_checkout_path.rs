//! Tree checkouts must not depend on the path taken. Minimal repros found by
//! the `loro.js` differential fuzz (loro-dev/loro#1141). See
//! `context/tree-checkout-window.md`.

use loro::{ExportMode, Frontiers, IdSpan, LoroDoc, TreeParentId, ID};

fn doc_with_peer(peer: u64) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer).unwrap();
    doc
}

fn merged(docs: &[&LoroDoc]) -> LoroDoc {
    let doc = LoroDoc::new();
    for d in docs {
        doc.import(&d.export(ExportMode::all_updates()).unwrap())
            .unwrap();
    }
    doc
}

fn tree_node_ids(doc: &LoroDoc, name: &str) -> Vec<String> {
    let tree = doc.get_tree(name);
    let mut ids: Vec<String> = tree
        .nodes()
        .into_iter()
        .filter(|id| !tree.is_node_deleted(id).unwrap())
        .map(|id| id.to_string())
        .collect();
    ids.sort();
    ids
}

/// With a subscriber, `checkout([3@1])` then `checkout([2@1])` panicked in
/// `TreeState::apply_diff_and_convert` (`get_index_by_tree_id(..).unwrap()` on
/// a `Create`).
#[test]
fn checkout_sequence_with_subscriber_does_not_panic() {
    let p1 = doc_with_peer(1);
    p1.get_list("l").insert(0, "x").unwrap();
    let tree = p1.get_tree("tr");
    let a = tree.create(TreeParentId::Root).unwrap();
    let b = tree.create(a).unwrap();
    p1.commit();

    let p3 = doc_with_peer(3);
    p3.import(
        &p1.export(ExportMode::updates_in_range(vec![IdSpan::new(1, 0, 1)]))
            .unwrap(),
    )
    .unwrap();
    p3.get_list("l").insert(0, "y").unwrap();
    p3.commit();

    let p2 = doc_with_peer(2);
    p2.import(&p3.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    p2.get_counter("c").increment(1.0).unwrap();
    p2.commit();

    p3.import(&p2.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    p3.get_map("m").insert("k", 1).unwrap();
    p3.commit();

    p2.import(&p1.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    p2.get_tree("tr")
        .get_meta(b)
        .unwrap()
        .insert("k", true)
        .unwrap();
    p2.commit();

    let doc = merged(&[&p1, &p2, &p3]);
    let _sub = doc.subscribe_root(std::sync::Arc::new(|_| {}));
    for f in [ID::new(3, 1), ID::new(2, 1)] {
        let f = Frontiers::from_id(f);
        doc.checkout(&f).unwrap();
        let fresh = merged(&[&p1, &p2, &p3]);
        fresh.checkout(&f).unwrap();
        assert_eq!(doc.get_deep_value(), fresh.get_deep_value(), "at {f:?}");
    }
}

/// Checking out `[3@2]` (which contains the creation of a tree node) and then
/// the concurrent `[1@1]` (which does not) kept the node.
#[test]
fn checkout_to_concurrent_version_without_node_creation_removes_node() {
    let p1 = doc_with_peer(1);
    let p2 = doc_with_peer(2);
    let p3 = doc_with_peer(3);
    p1.get_counter("c").increment(1.0).unwrap();
    p1.commit(); // 0@1
    p3.import(&p1.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    p3.get_tree("tr").create(TreeParentId::Root).unwrap();
    p3.commit(); // 0@3, deps 0@1
    p2.get_list("l").insert(0, 1).unwrap();
    p2.get_list("l").delete(0, 1).unwrap();
    p2.commit(); // 0@2..1@2
    p2.import(&p1.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    p2.get_counter("c").increment(1.0).unwrap();
    p2.commit(); // 2@2, deps [0@1, 1@2]
    p1.import(&p2.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    p1.get_map("m").insert("k", 1).unwrap();
    p1.commit(); // 1@1, deps 2@2
    p2.import(&p3.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    p2.get_text("t").insert(0, "x").unwrap();
    p2.commit(); // 3@2, deps [2@2, 0@3]

    let target = Frontiers::from_id(ID::new(1, 1));
    let direct = merged(&[&p1, &p2, &p3]);
    direct.checkout(&target).unwrap();
    assert!(tree_node_ids(&direct, "tr").is_empty());

    let doc = merged(&[&p1, &p2, &p3]);
    doc.checkout(&Frontiers::from_id(ID::new(2, 3))).unwrap();
    assert_eq!(tree_node_ids(&doc, "tr").len(), 1);
    doc.checkout(&target).unwrap();
    assert_eq!(tree_node_ids(&doc, "tr"), tree_node_ids(&direct, "tr"));
    assert_eq!(doc.get_deep_value(), direct.get_deep_value());
}
