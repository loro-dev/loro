use criterion::{criterion_group, criterion_main, Criterion};
#[cfg(feature = "test_utils")]
mod tree {
    use super::*;
    use loro_internal::LoroDoc;
    use rand::{rngs::StdRng, Rng};

    pub fn tree_move(c: &mut Criterion) {
        let mut b = c.benchmark_group("movable tree");
        b.sample_size(10);
        b.bench_function("10^3 tree move 10^5", |b| {
            let loro = LoroDoc::default();
            let tree = loro.get_tree("tree");
            let mut ids = vec![];
            let size = 1000;
            for _ in 0..size {
                ids.push(
                    loro.with_txn(|txn| tree.create_with_txn(txn, None))
                        .unwrap(),
                )
            }
            let mut rng: StdRng = rand::SeedableRng::seed_from_u64(0);
            let n = 100000;
            b.iter(|| {
                let mut txn = loro.txn().unwrap();
                for _ in 0..n {
                    let i = rng.gen::<usize>() % size;
                    let j = rng.gen::<usize>() % size;
                    tree.mov_with_txn(&mut txn, ids[i], ids[j])
                        .unwrap_or_default();
                }
                drop(txn)
            })
        });

        b.bench_function("1000 node checkout 10^3", |b| {
            let mut loro = LoroDoc::default();
            let tree = loro.get_tree("tree");
            let mut ids = vec![];
            let mut versions = vec![];
            let size = 1000;
            for _ in 0..size {
                ids.push(
                    loro.with_txn(|txn| tree.create_with_txn(txn, None))
                        .unwrap(),
                )
            }
            let mut rng: StdRng = rand::SeedableRng::seed_from_u64(0);
            let mut n = 1000;
            while n > 0 {
                let i = rng.gen::<usize>() % size;
                let j = rng.gen::<usize>() % size;
                if loro
                    .with_txn(|txn| tree.mov_with_txn(txn, ids[i], ids[j]))
                    .is_ok()
                {
                    versions.push(loro.oplog_frontiers());
                    n -= 1;
                };
            }
            b.iter(|| {
                for _ in 0..1000 {
                    let i = rng.gen::<usize>() % 1000;
                    let f = &versions[i];
                    loro.checkout(f).unwrap();
                }
            })
        });

        b.bench_function("300 deep node random checkout 10^3", |b| {
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
            b.iter(|| {
                for _ in 0..1000 {
                    let i = rng.gen::<usize>() % depth;
                    let f = &versions[i];
                    loro.checkout(f).unwrap();
                }
            })
        });
    }
}

pub fn dumb(_c: &mut Criterion) {}

#[cfg(feature = "test_utils")]
criterion_group!(benches, tree::tree_move);
#[cfg(not(feature = "test_utils"))]
criterion_group!(benches, dumb);
criterion_main!(benches);
