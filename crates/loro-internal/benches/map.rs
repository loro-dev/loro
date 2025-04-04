use criterion::{criterion_group, criterion_main, Criterion};
#[cfg(feature = "test_utils")]
mod map {
    use super::*;
    use loro_internal::LoroDoc;
    use rand::{rngs::StdRng, Rng};

    pub fn tree_move(c: &mut Criterion) {
        let mut b = c.benchmark_group("map crdt");
        b.sample_size(10);
        b.bench_function("create 10^4 key", |b| {
            let size = 10000;
            b.iter(|| {
                let loro = LoroDoc::new_auto_commit();
                let map = loro.get_map("map");
                for i in 0..size {
                    map.insert(&i.to_string(), i).unwrap();
                }
                loro.commit_then_renew();
            })
        });

        b.bench_function("map checkout 10^3", |b| {
            let loro = LoroDoc::new_auto_commit();
            let map = loro.get_map("map");
            let mut versions = vec![];
            let size = 10000;
            let mut rng: StdRng = rand::SeedableRng::seed_from_u64(0);
            for i in 0..size {
                versions.push(loro.oplog_frontiers());
                map.insert(&rng.gen::<u8>().to_string(), i).unwrap();
                loro.commit_then_renew();
            }

            b.iter(|| {
                for _ in 0..1000 {
                    let i = rng.gen::<usize>() % 1000;
                    let f = &versions[i];
                    loro.checkout(f).unwrap();
                }
            })
        });

        b.bench_function("realtime map set", |b| {
            let doc_a = LoroDoc::default();
            let doc_b = LoroDoc::default();
            let map_a = doc_a.get_map("map");
            let map_b = doc_b.get_map("map");
            let n = 1000;
            b.iter(|| {
                for t in 0..n {
                    if t % 2 == 0 {
                        let mut txn = doc_a.txn().unwrap();
                        map_a.insert_with_txn(&mut txn, "key", t.into()).unwrap();
                        doc_b.import(&doc_a.export_from(&doc_b.oplog_vv())).unwrap();
                    } else {
                        let mut txn = doc_b.txn().unwrap();
                        map_b.insert_with_txn(&mut txn, "key", t.into()).unwrap();
                        doc_a.import(&doc_b.export_from(&doc_a.oplog_vv())).unwrap();
                    }
                }
            })
        });
    }
}

pub fn dumb(_c: &mut Criterion) {}

#[cfg(feature = "test_utils")]
criterion_group!(benches, map::tree_move);
#[cfg(not(feature = "test_utils"))]
criterion_group!(benches, dumb);
criterion_main!(benches);
