use std::time::Instant;

use loro_internal::LoroDoc;
use rand::{rngs::StdRng, Rng};

fn checkout() {
    let depth = 300;
    let mut loro = LoroDoc::default();
    let tree = loro.get_tree("tree");
    let mut ids = vec![];
    let mut versions = vec![];
    let id1 = loro
        .with_txn(|txn| tree.create_with_txn(txn, None))
        .unwrap();
    ids.push(id1);
    versions.push(loro.oplog_frontiers());
    for _ in 1..depth {
        let id = loro
            .with_txn(|txn| tree.create_with_txn(txn, *ids.last().unwrap()))
            .unwrap();
        ids.push(id);
        versions.push(loro.oplog_frontiers());
    }
    let mut rng: StdRng = rand::SeedableRng::seed_from_u64(0);

    for _ in 0..1000 {
        let i = rng.gen::<usize>() % depth;
        let f = &versions[i];
        loro.checkout(f).unwrap();
    }
}

#[allow(unused)]
fn mov() {
    let loro = LoroDoc::default();
    let tree = loro.get_tree("tree");
    let mut ids = vec![];
    let size = 10000;
    for _ in 0..size {
        ids.push(
            loro.with_txn(|txn| tree.create_with_txn(txn, None))
                .unwrap(),
        )
    }
    let mut rng: StdRng = rand::SeedableRng::seed_from_u64(0);
    let n = 1000000;

    let mut txn = loro.txn().unwrap();
    for _ in 0..n {
        let i = rng.gen::<usize>() % size;
        let j = rng.gen::<usize>() % size;
        tree.mov_with_txn(&mut txn, ids[i], ids[j])
            .unwrap_or_default();
    }
    drop(txn);
}
fn main() {
    let s = Instant::now();
    for _ in 0..30 {
        checkout();
    }

    println!("{} ms", s.elapsed().as_millis());
}
