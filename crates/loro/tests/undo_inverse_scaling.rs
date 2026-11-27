//! Undo bookkeeping that should stay linear in the number of touched items.

use loro::{event::Diff, undo::DiffBatch, LoroDoc, TreeParentId, UndoManager};
use std::time::Instant;

/// Rebasing an undo item over a remote tree diff keeps one item per node,
/// the last one, and drops the nodes the remote diff also moved.
#[test]
fn tree_diff_transform_keeps_last_item_per_node() {
    for n in [1_000usize, 4_000] {
        let doc = LoroDoc::new();
        let tree = doc.get_tree("tree");
        tree.enable_fractional_index(0);
        let p = tree.create(TreeParentId::Root).unwrap();
        let q = tree.create(TreeParentId::Root).unwrap();
        let spare = tree.create(TreeParentId::Root).unwrap();
        let nodes: Vec<_> = (0..n).map(|_| tree.create(p).unwrap()).collect();
        doc.commit();

        let mut acc = DiffBatch::default();
        for parent in [q, p] {
            let before = doc.state_frontiers();
            for &node in &nodes {
                tree.mov(node, parent).unwrap();
            }
            doc.commit();
            let delta: DiffBatch = doc.diff(&before, &doc.state_frontiers()).unwrap().into();
            acc.compose(&delta);
        }

        // The other side moves `spare` and the first node.
        let before = doc.state_frontiers();
        tree.mov(spare, q).unwrap();
        tree.mov(nodes[0], q).unwrap();
        doc.commit();
        let other: DiffBatch = doc.diff(&before, &doc.state_frontiers()).unwrap().into();

        let start = Instant::now();
        acc.transform(&other, false);
        eprintln!("tree nodes={n} transform={:?}", start.elapsed());

        let out: loro::event::DiffBatch = acc.into();
        let (_, diff) = out.iter().next().unwrap();
        let Diff::Tree(tree_diff) = diff else {
            panic!("expected a tree diff")
        };
        let targets: Vec<_> = tree_diff.diff.iter().map(|x| x.target).collect();
        assert_eq!(targets, nodes[1..].to_vec());
        for item in tree_diff.diff.iter() {
            match &item.action {
                loro::TreeExternalDiff::Move { parent, .. } => {
                    assert_eq!(*parent, TreeParentId::Node(p))
                }
                other => panic!("unexpected action {other:?}"),
            }
        }
    }
}

/// One commit that touches many movable lists undoes in one step.
#[test]
fn one_commit_over_many_movable_lists_undoes_at_once() {
    for n in [1_000usize, 4_000] {
        let doc = LoroDoc::new();
        let lists: Vec<_> = (0..n)
            .map(|i| {
                let list = doc.get_movable_list(format!("list_{i}").as_str());
                list.push(0).unwrap();
                list
            })
            .collect();
        doc.commit();
        let mut undo = UndoManager::new(&doc);
        for list in &lists {
            list.push(1).unwrap();
        }
        let start = Instant::now();
        doc.commit();
        eprintln!("movable lists={n} commit={:?}", start.elapsed());
        assert!(lists.iter().all(|l| l.len() == 2));
        assert!(undo.undo().unwrap());
        assert!(lists.iter().all(|l| l.len() == 1));
    }
}
