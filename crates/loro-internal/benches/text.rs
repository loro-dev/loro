use criterion::{criterion_group, criterion_main, Criterion};

#[cfg(feature = "test_utils")]
mod run {

    use super::*;
    use arbitrary::Unstructured;
    use bench_utils::TextAction;
    use loro_internal::container::registry::ContainerWrapper;
    use loro_internal::fuzz::test_multi_sites;
    use loro_internal::fuzz::Action;
    use loro_internal::LoroCore;
    use loro_internal::Transact;
    use rand::Rng;
    use rand::SeedableRng;

    pub fn two_client_edits(c: &mut Criterion) {
        let mut rgn = rand::rngs::StdRng::seed_from_u64(0);
        let mut bytes = Vec::new();
        for _ in 0..8000 {
            bytes.push(rgn.gen::<u8>());
        }

        let mut gen = Unstructured::new(&bytes);
        let mut c = c.benchmark_group("sync");
        let mut actions = gen.arbitrary::<[Action; 200]>().unwrap();
        c.bench_function("random text edit 2 sites", |b| {
            b.iter(|| test_multi_sites(2, &mut actions))
        });

        c.bench_function("random text edit 8 sites", |b| {
            b.iter(|| test_multi_sites(8, &mut actions))
        });
        let mut actions = gen.arbitrary::<[Action; 4000]>().unwrap();
        c.sample_size(10);
        c.bench_function("random text edit 8 sites long", |b| {
            b.iter(|| test_multi_sites(8, &mut actions))
        });
    }

    pub fn b4(c: &mut Criterion) {
        let actions = bench_utils::get_automerge_actions();
        let mut b = c.benchmark_group("direct_apply");
        b.sample_size(10);
        b.bench_function("B4", |b| {
            b.iter(|| {
                let mut loro = LoroCore::default();
                let mut text = loro.get_text("text");

                for TextAction { pos, ins, del } in actions.iter() {
                    text.delete(&loro, *pos, *del).unwrap();
                    text.insert(&loro, *pos, ins).unwrap();
                }
            })
        });

        b.bench_function("B4_Per100_Txn", |b| {
            b.iter(|| {
                let mut loro = LoroCore::default();
                let mut text = loro.get_text("text");
                let mut n = 0;
                let mut txn = loro.transact();
                for TextAction { pos, ins, del } in actions.iter() {
                    if n == 100 {
                        n = 0;
                        drop(txn);
                        txn = loro.transact();
                    }
                    n += 1;
                    text.delete(&txn, *pos, *del).unwrap();
                    text.insert(&txn, *pos, ins).unwrap();
                }
            })
        });

        b.bench_function("B4_All_Txn", |b| {
            b.iter(|| {
                let mut loro = LoroCore::default();
                let mut text = loro.get_text("text");
                {
                    let txn = loro.transact();
                    for TextAction { pos, ins, del } in actions.iter() {
                        text.delete(&txn, *pos, *del).unwrap();
                        text.insert(&txn, *pos, ins).unwrap();
                    }
                }
            })
        });

        b.bench_function("B4 Observed", |b| {
            b.iter(|| {
                let mut loro = LoroCore::default();
                loro.subscribe_deep(Box::new(|_| {}));
                let mut text = loro.get_text("text");
                {
                    let txn = loro.transact();
                    for TextAction { pos, ins, del } in actions.iter() {
                        text.delete(&txn, *pos, *del).unwrap();
                        text.insert(&txn, *pos, ins).unwrap();
                    }
                }
            })
        });

        b.bench_function("B4 Observed_Per100_Txn", |b| {
            b.iter(|| {
                let mut loro = LoroCore::default();
                loro.subscribe_deep(Box::new(|_| {}));
                let mut text = loro.get_text("text");
                let mut n = 0;
                let mut txn = loro.transact();
                for TextAction { pos, ins, del } in actions.iter() {
                    if n == 100 {
                        n = 0;
                        drop(txn);
                        txn = loro.transact();
                    }
                    n += 1;
                    text.delete(&txn, *pos, *del).unwrap();
                    text.insert(&txn, *pos, ins).unwrap();
                }
            })
        });

        b.bench_function("B4 Observed_All_Txn", |b| {
            b.iter(|| {
                let mut loro = LoroCore::default();
                loro.subscribe_deep(Box::new(|_| {}));
                let mut text = loro.get_text("text");
                {
                    let txn = loro.transact();
                    for TextAction { pos, ins, del } in actions.iter() {
                        text.delete(&txn, *pos, *del).unwrap();
                        text.insert(&txn, *pos, ins).unwrap();
                    }
                }
            })
        });

        // b.sample_size(10);
        b.bench_function("B4DirectSync", |b| {
            b.iter(|| {
                let mut loro = LoroCore::default();
                let mut loro_b = LoroCore::default();
                let mut text = loro.get_text("text");
                for TextAction { pos, ins, del } in actions.iter() {
                    {
                        let txn = loro.transact();
                        text.delete(&txn, *pos, *del).unwrap();
                        text.insert(&txn, *pos, ins).unwrap();
                    }

                    loro_b.import(loro.export(loro_b.vv_cloned()));
                }
            })
        });

        drop(b);
        let mut b = c.benchmark_group("sync");
        b.bench_function("B4Parallel", |b| {
            b.iter(|| {
                let mut loro = LoroCore::default();
                let mut loro_b = LoroCore::default();
                let mut text = loro.get_text("text");
                let mut text2 = loro_b.get_text("text");
                let mut i = 0;
                for TextAction { pos, ins, del } in actions.iter() {
                    let pos = *pos;
                    let del = *del;
                    i += 1;
                    if i > 1000 {
                        break;
                    }

                    {
                        let txn = loro.transact();
                        text.delete(&txn, pos, del).unwrap();
                        text.insert(&txn, pos, ins).unwrap();
                    }

                    {
                        let txn = loro_b.transact();
                        text2.delete(&txn, pos, del).unwrap();
                        text2.insert(&txn, pos, ins).unwrap();
                    }
                    loro_b.import(loro.export(loro_b.vv_cloned()));
                    loro.import(loro_b.export(loro.vv_cloned()));
                }
            })
        });
    }
}
pub fn dumb(_c: &mut Criterion) {}

#[cfg(feature = "test_utils")]
criterion_group!(benches, run::two_client_edits, run::b4);
#[cfg(not(feature = "test_utils"))]
criterion_group!(benches, dumb);
criterion_main!(benches);
