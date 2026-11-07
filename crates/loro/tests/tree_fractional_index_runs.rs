//! Siblings a peer inserts one after another keep together when another peer inserts its own
//! run into the same gap concurrently (fractional index with jitter).
use loro::{ExportMode, LoroDoc, LoroTree, TreeID, TreeParentId};
use rand::{rngs::StdRng, Rng, SeedableRng};

const JITTER: u8 = 8;

fn doc(peer: u64) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer).unwrap();
    doc.get_tree("tree").enable_fractional_index(JITTER);
    doc
}

fn sync(a: &LoroDoc, b: &LoroDoc) {
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
}

fn roots(tree: &LoroTree) -> Vec<TreeID> {
    tree.children(TreeParentId::Root).unwrap_or_default()
}

/// Inserts `n` siblings one after another, right after `after`.
fn run(tree: &LoroTree, after: TreeID, n: usize) -> Vec<TreeID> {
    let mut ids = Vec::new();
    let mut left = after;
    for _ in 0..n {
        let index = roots(tree).iter().position(|id| *id == left).unwrap() + 1;
        left = tree.create_at(TreeParentId::Root, index).unwrap();
        ids.push(left);
    }
    ids
}

fn contiguous(order: &[TreeID], ids: &[TreeID]) -> bool {
    let start = order.iter().position(|id| *id == ids[0]).unwrap();
    order[start..start + ids.len()] == *ids
}

fn assert_sorted_indexes(tree: &LoroTree) {
    let indexes: Vec<String> = roots(tree)
        .iter()
        .map(|id| tree.fractional_index(*id).unwrap())
        .collect();
    assert!(
        indexes.windows(2).all(|w| w[0] <= w[1]),
        "children out of index order: {indexes:?}"
    );
}

#[test]
fn concurrent_runs_in_one_gap_do_not_interleave() {
    let a = doc(1);
    let ta = a.get_tree("tree");
    let intro = ta.create(TreeParentId::Root).unwrap();
    let _outro = ta.create(TreeParentId::Root).unwrap();
    let b = doc(2);
    sync(&a, &b);
    let tb = b.get_tree("tree");

    let mine = run(&ta, intro, 5);
    let theirs = run(&tb, intro, 5);
    sync(&a, &b);

    let order = roots(&ta);
    assert_eq!(order, roots(&tb));
    assert!(contiguous(&order, &mine), "{order:?}");
    assert!(contiguous(&order, &theirs), "{order:?}");
    assert_sorted_indexes(&ta);
}

#[test]
fn another_peer_can_insert_inside_a_run() {
    let a = doc(1);
    let ta = a.get_tree("tree");
    let first = ta.create(TreeParentId::Root).unwrap();
    let mine = run(&ta, first, 4);
    let b = doc(2);
    sync(&a, &b);
    let tb = b.get_tree("tree");

    // Between the anchor and its first extension, and between two extensions.
    for index in [1, 3] {
        tb.create_at(TreeParentId::Root, index).unwrap();
    }
    sync(&a, &b);

    assert_eq!(roots(&ta), roots(&tb));
    assert_eq!(roots(&ta).len(), 7);
    assert!(roots(&ta).ends_with(&mine[3..]));
    assert_sorted_indexes(&ta);
}

#[test]
fn a_long_run_keeps_its_indexes_short() {
    let a = doc(1);
    let ta = a.get_tree("tree");
    let first = ta.create(TreeParentId::Root).unwrap();
    run(&ta, first, 1000);
    let longest = roots(&ta)
        .iter()
        .map(|id| ta.fractional_index(*id).unwrap().len() / 2)
        .max()
        .unwrap();
    assert!(longest <= 32, "longest index {longest} bytes");
    assert_sorted_indexes(&ta);
}

/// Peers insert anywhere — runs, single inserts, inside each other's runs — syncing at
/// random; every index is generated without panicking, and all peers converge on one order
/// that matches the indexes.
#[test]
fn random_inserts_converge_without_panicking() {
    for seed in 0..64u64 {
        let mut rng = StdRng::seed_from_u64(seed);
        let docs: Vec<LoroDoc> = (1..=3).map(doc).collect();
        let trees: Vec<LoroTree> = docs.iter().map(|d| d.get_tree("tree")).collect();
        trees[0].create(TreeParentId::Root).unwrap();
        for d in &docs[1..] {
            sync(&docs[0], d);
        }
        let mut last: Vec<Option<TreeID>> = vec![None; 3];
        for _ in 0..60 {
            let p = rng.gen_range(0..3);
            let tree = &trees[p];
            let len = roots(tree).len();
            let index = match last[p] {
                Some(prev) if rng.gen_bool(0.6) => roots(tree)
                    .iter()
                    .position(|id| *id == prev)
                    .map_or(len, |i| i + 1),
                _ => rng.gen_range(0..=len),
            };
            last[p] = Some(tree.create_at(TreeParentId::Root, index).unwrap());
            if rng.gen_bool(0.2) {
                let (x, y) = (rng.gen_range(0..3), rng.gen_range(0..3));
                if x != y {
                    sync(&docs[x], &docs[y]);
                }
            }
        }
        for _ in 0..2 {
            sync(&docs[0], &docs[1]);
            sync(&docs[1], &docs[2]);
        }
        let order = roots(&trees[0]);
        assert_eq!(order, roots(&trees[1]), "seed {seed}");
        assert_eq!(order, roots(&trees[2]), "seed {seed}");
        assert_sorted_indexes(&trees[0]);
    }
}
