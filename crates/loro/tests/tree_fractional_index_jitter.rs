use loro::{LoroDoc, TreeParentId};

/// loro-dev/loro#1211: with jitter, a sibling's fractional index ends with a
/// random byte, which can be 0xFF or sit next to a neighbour's last byte.
/// Creating a node between such siblings used to panic.
#[test]
fn tree_insert_between_jittered_siblings() {
    let mut seed: u64 = 1;
    let mut rand = || {
        seed = (seed * 1103515245 + 12345) % (1 << 31);
        seed as f64 / (1u64 << 31) as f64
    };
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let tree = doc.get_tree("t");
    tree.enable_fractional_index(1);
    let root = tree.create(TreeParentId::Root).unwrap();
    for _ in 0..1000 {
        let kids = tree.children(root).unwrap_or_default();
        if kids.len() > 40 && rand() < 0.4 {
            tree.delete(kids[(rand() * kids.len() as f64) as usize])
                .unwrap();
        } else {
            let index = (rand() * (kids.len() + 1) as f64) as usize;
            let node = tree.create_at(root, index).unwrap();
            let kids = tree.children(root).unwrap();
            assert_eq!(kids[index], node);
            let positions: Vec<_> = kids
                .iter()
                .map(|&id| tree.fractional_index(id).unwrap())
                .collect();
            assert!(positions.windows(2).all(|w| w[0] < w[1]), "{positions:?}");
        }
        doc.commit();
    }
}

/// loro-dev/loro#1289: jitter is meant to separate concurrent inserts at the
/// same position, so peers must not draw the same jitter bytes. The default
/// tree config used to seed its RNG with 0 on every peer.
#[test]
fn tree_jitter_differs_between_peers() {
    let docs: Vec<LoroDoc> = [1, 2, 0xfedc_ba98_7654_3210]
        .into_iter()
        .map(|peer| {
            let doc = LoroDoc::new();
            doc.set_peer_id(peer).unwrap();
            doc.get_tree("tree").enable_fractional_index(4);
            doc.get_tree("tree").create(TreeParentId::Root).unwrap();
            doc.commit();
            doc
        })
        .collect();
    let positions: Vec<String> = docs
        .iter()
        .map(|doc| {
            let tree = doc.get_tree("tree");
            tree.fractional_index(tree.roots()[0]).unwrap()
        })
        .collect();
    for (i, a) in positions.iter().enumerate() {
        for b in &positions[i + 1..] {
            assert_ne!(a, b, "{positions:?}");
        }
    }

    // After merging, the concurrent roots keep distinct positions.
    let merged = LoroDoc::new();
    for doc in &docs {
        merged
            .import(&doc.export(loro::ExportMode::all_updates()).unwrap())
            .unwrap();
    }
    let tree = merged.get_tree("tree");
    let mut merged_positions: Vec<String> = tree
        .roots()
        .into_iter()
        .map(|id| tree.fractional_index(id).unwrap())
        .collect();
    merged_positions.sort();
    merged_positions.dedup();
    assert_eq!(merged_positions.len(), 3);
}

/// The jitter RNG is seeded from the peer, so a doc with a fixed peer id
/// produces the same positions on every run.
#[test]
fn tree_jitter_is_deterministic_for_a_peer() {
    let position = || {
        let doc = LoroDoc::new();
        doc.set_peer_id(7).unwrap();
        let tree = doc.get_tree("tree");
        tree.enable_fractional_index(4);
        let node = tree.create(TreeParentId::Root).unwrap();
        tree.fractional_index(node).unwrap()
    };
    assert_eq!(position(), position());
}
