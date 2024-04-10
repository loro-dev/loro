use std::time::Instant;

use loro_internal::LoroDoc;
use rand::{rngs::StdRng, Rng};

#[allow(unused)]
fn checkout() {
    let depth = 300;
    let loro = LoroDoc::default();
    let tree = loro.get_tree("tree");
    let mut ids = vec![];
    let mut versions = vec![];
    let id1 = loro
        .with_txn(|txn| tree.create_with_txn(txn, None, 0))
        .unwrap();
    ids.push(id1);
    versions.push(loro.oplog_frontiers());
    for _ in 1..depth {
        let id = loro
            .with_txn(|txn| tree.create_with_txn(txn, *ids.last().unwrap(), 0))
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
    let loro = LoroDoc::new_auto_commit();
    let tree = loro.get_tree("tree");
    let mut ids = vec![];
    let size = 10000;
    for _ in 0..size {
        ids.push(tree.create_at(None, 0).unwrap())
    }
    let mut rng: StdRng = rand::SeedableRng::seed_from_u64(0);
    let n = 100000;

    for _ in 0..n {
        let i = rng.gen::<usize>() % size;
        let j = rng.gen::<usize>() % size;
        let children_num = tree.children_num(Some(ids[j])).unwrap_or(0);
        tree.move_to(ids[i], ids[j], children_num)
            .unwrap_or_default();
    }
    println!("encode snapshot size {:?}", loro.export_snapshot().len());
    println!(
        "encode updates size {:?}",
        loro.export_from(&Default::default()).len()
    );
}

#[allow(unused)]
fn create() {
    let size = 10000;
    let loro = LoroDoc::default();
    let tree = loro.get_tree("tree");
    for _ in 0..size {
        loro.with_txn(|txn| tree.create_with_txn(txn, None, 0))
            .unwrap();
    }
}

fn main() {
    let s = Instant::now();
    mov();
    println!("{} ms", s.elapsed().as_millis());
}
