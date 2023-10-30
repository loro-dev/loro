use std::time::Instant;

use loro_internal::LoroDoc;
use rand::{rngs::StdRng, Rng};

fn main() {
    let s = Instant::now();
    let loro = LoroDoc::default();
    let tree = loro.get_tree("tree");
    let mut ids = vec![];
    let size = 10000;
    for _ in 0..size {
        ids.push(loro.with_txn(|txn| tree.create(txn)).unwrap())
    }
    let mut rng: StdRng = rand::SeedableRng::seed_from_u64(0);
    let n = 1000000;

    let mut txn = loro.txn().unwrap();
    for _ in 0..n {
        let i = rng.gen::<usize>() % size;
        let j = rng.gen::<usize>() % size;
        tree.mov(&mut txn, ids[i], ids[j]).unwrap_or_default();
    }
    drop(txn);
    println!("{} ms", s.elapsed().as_millis());
}
