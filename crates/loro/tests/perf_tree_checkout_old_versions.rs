use loro::{Frontiers, LoroDoc, TreeID, TreeParentId, ID};
use std::time::{Duration, Instant};

/// A tree with 2k nodes and `moves` random moves by one peer, committed every
/// 500 moves.
fn tree_history(moves: usize) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let tree = doc.get_tree("tree");
    let mut nodes: Vec<TreeID> = Vec::new();
    let mut seed = 1u64;
    let mut next = move |n: usize| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as usize % n
    };
    for i in 0..2_000 {
        let parent = if i < 20 {
            TreeParentId::Root
        } else {
            TreeParentId::Node(nodes[next(nodes.len())])
        };
        nodes.push(tree.create(parent).unwrap());
    }
    doc.commit();
    for i in 0..moves {
        let target = nodes[next(nodes.len())];
        let parent = nodes[next(nodes.len())];
        let _ = tree.mov(target, parent);
        if i % 500 == 499 {
            doc.commit();
        }
    }
    doc.commit();
    doc
}

/// Checking out back and forth between two old versions, 40 checkouts.
fn scrub_old_versions(doc: &LoroDoc) -> Duration {
    let a = Frontiers::from_id(ID::new(1, 2_500));
    let b = Frontiers::from_id(ID::new(1, 2_510));
    doc.checkout(&a).unwrap();
    doc.checkout(&b).unwrap();
    let start = Instant::now();
    for _ in 0..20 {
        doc.checkout(&a).unwrap();
        doc.checkout(&b).unwrap();
    }
    start.elapsed()
}

/// The tree diff retreats only the ops the cached version holds, so checking
/// out between two old versions must not get slower as later history grows
/// (it scanned every later tree op before `TreeCacheForDiff::max_lamport`).
/// See `context/tree-checkout-window.md`.
#[test]
#[ignore]
fn perf_tree_checkout_between_old_versions_does_not_scale_with_later_history() {
    // Run with:
    // cargo test --release -p loro --test perf_tree_checkout_old_versions -- --ignored --nocapture
    let short = tree_history(10_000);
    let long = tree_history(200_000);
    let (mut t_short, mut t_long) = (Duration::MAX, Duration::MAX);
    for _ in 0..3 {
        t_short = t_short.min(scrub_old_versions(&short));
        t_long = t_long.min(scrub_old_versions(&long));
    }
    println!("10k later moves: {t_short:?}, 200k later moves: {t_long:?}");
    assert!(
        t_long < t_short * 4 + Duration::from_millis(5),
        "checkout between old versions scales with later history: {t_short:?} -> {t_long:?}"
    );
}
