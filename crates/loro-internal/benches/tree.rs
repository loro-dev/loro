use criterion::{criterion_group, criterion_main, Criterion};

mod tree {
    use super::*;
    use criterion::{AxisScale, BenchmarkId, PlotConfiguration};
    use loro_internal::LoroDoc;
    use rand::{rngs::StdRng, Rng};

    pub fn tree_move(c: &mut Criterion) {
        let mut group = c.benchmark_group("movable tree");
        let plot_config = PlotConfiguration::default().summary_scale(AxisScale::Logarithmic);
        group.plot_config(plot_config);
        group.sample_size(10);

        for i in 3..=6 {
            let input = 10u64.pow(i);
            group.bench_with_input(
                BenchmarkId::new("create node append", input),
                &input,
                |b, i| {
                    b.iter(|| {
                        let loro = LoroDoc::new_auto_commit();
                        let tree = loro.get_tree("tree");
                        for idx in 0..*i {
                            tree.create_at(None, idx as usize).unwrap();
                        }
                    })
                },
            );

            group.bench_with_input(
                BenchmarkId::new("create node front", input),
                &input,
                |b, i| {
                    b.iter(|| {
                        let loro = LoroDoc::new_auto_commit();
                        let tree = loro.get_tree("tree");
                        for _ in 0..*i {
                            tree.create_at(None, 0).unwrap();
                        }
                    })
                },
            );

            group.bench_with_input(
                BenchmarkId::new("move node append", input),
                &input,
                |b, i| {
                    let loro = LoroDoc::new_auto_commit();
                    let tree = loro.get_tree("tree");
                    const SIZE: usize = 1000;
                    let mut rng: StdRng = rand::SeedableRng::seed_from_u64(0);
                    let mut ids = vec![];
                    for _ in 0..SIZE {
                        let pos = rng.gen::<usize>() % (ids.len() + 1);
                        ids.push(tree.create_at(None, pos).unwrap());
                    }

                    b.iter(|| {
                        for _ in 0..*i {
                            tree.create_at(None, 0).unwrap();
                            let i = rng.gen::<usize>() % SIZE;
                            let j = rng.gen::<usize>() % SIZE;
                            tree.mov(ids[i], ids[j]).unwrap_or_default();
                        }
                    })
                },
            );

            group.bench_with_input(
                BenchmarkId::new("move node front", input),
                &input,
                |b, i| {
                    let loro = LoroDoc::new_auto_commit();
                    let tree = loro.get_tree("tree");
                    const SIZE: usize = 1000;
                    let mut rng: StdRng = rand::SeedableRng::seed_from_u64(0);
                    let mut ids = vec![];
                    for _ in 0..SIZE {
                        let pos = rng.gen::<usize>() % (ids.len() + 1);
                        ids.push(tree.create_at(None, pos).unwrap());
                    }

                    b.iter(|| {
                        for _ in 0..*i {
                            tree.create_at(None, 0).unwrap();
                            let i = rng.gen::<usize>() % SIZE;
                            let j = rng.gen::<usize>() % SIZE;
                            tree.move_to(ids[i], ids[j], 0).unwrap_or_default();
                        }
                    })
                },
            );
        }

        group.bench_function("1000 node checkout 10^3", |b| {
            let loro = LoroDoc::default();
            let tree = loro.get_tree("tree");
            let mut ids = vec![];
            let mut versions = vec![];
            let size = 1000;
            for _ in 0..size {
                ids.push(
                    loro.with_txn(|txn| tree.create_with_txn(txn, None, 0))
                        .unwrap(),
                )
            }
            let mut rng: StdRng = rand::SeedableRng::seed_from_u64(0);
            let mut n = 1000;
            while n > 0 {
                let i = rng.gen::<usize>() % size;
                let j = rng.gen::<usize>() % size;
                if loro
                    .with_txn(|txn| tree.mov_with_txn(txn, ids[i], ids[j], 0))
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

        group.bench_function("300 deep node random checkout 10^3", |b| {
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
            b.iter(|| {
                for _ in 0..1000 {
                    let i = rng.gen::<usize>() % depth;
                    let f = &versions[i];
                    loro.checkout(f).unwrap();
                }
            })
        });

        group.bench_function("realtime tree move", |b| {
            let doc_a = LoroDoc::default();
            let doc_b = LoroDoc::default();
            let tree_a = doc_a.get_tree("tree");
            let tree_b = doc_b.get_tree("tree");
            let mut ids = vec![];
            let size = 1000;
            for _ in 0..size {
                ids.push(
                    doc_a
                        .with_txn(|txn| tree_a.create_with_txn(txn, None, 0))
                        .unwrap(),
                )
            }
            doc_b.import(&doc_a.export_snapshot()).unwrap();
            let mut rng: StdRng = rand::SeedableRng::seed_from_u64(0);
            let n = 1000;
            b.iter(|| {
                for t in 0..n {
                    let i = rng.gen::<usize>() % size;
                    let j = rng.gen::<usize>() % size;
                    if t % 2 == 0 {
                        let mut txn = doc_a.txn().unwrap();
                        tree_a
                            .mov_with_txn(&mut txn, ids[i], ids[j], 0)
                            .unwrap_or_default();
                        doc_b.import(&doc_a.export_from(&doc_b.oplog_vv())).unwrap();
                    } else {
                        let mut txn = doc_b.txn().unwrap();
                        tree_b
                            .mov_with_txn(&mut txn, ids[i], ids[j], 0)
                            .unwrap_or_default();
                        doc_a.import(&doc_b.export_from(&doc_a.oplog_vv())).unwrap();
                    }
                }
            })
        });
        group.finish();
    }
}

criterion_group!(benches, tree::tree_move);
criterion_main!(benches);
