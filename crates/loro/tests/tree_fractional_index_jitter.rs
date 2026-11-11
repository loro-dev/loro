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
