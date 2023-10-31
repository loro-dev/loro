use criterion::{criterion_group, criterion_main, Criterion};

#[cfg(feature = "test_utils")]
mod run {
    use std::sync::Arc;

    use super::*;
    use bench_utils::TextAction;
    use criterion::black_box;
    use loro_common::LoroValue;
    use loro_internal::loro::LoroDoc;

    pub fn b4(c: &mut Criterion) {
        let actions = bench_utils::get_automerge_actions();
        let mut b = c.benchmark_group("direct_apply");
        b.sample_size(10);
        b.bench_function("B4", |b| {
            b.iter(|| {
                let loro = LoroDoc::default();
                let text = loro.get_text("text");
                let mut txn = loro.txn().unwrap();

                for TextAction { pos, ins, del } in actions.iter() {
                    text.delete(&mut txn, *pos, *del).unwrap();
                    text.insert(&mut txn, *pos, ins).unwrap();
                }
            })
        });

        b.bench_function("B4 with 100K actors history", |b| {
            let store = LoroDoc::default();
            for i in 0..100_000 {
                store.set_peer_id(i).unwrap();
                let list = store.get_list("list");
                let value: LoroValue = i.to_string().into();
                let mut txn = store.txn().unwrap();
                list.insert(&mut txn, 0, value).unwrap();
                txn.commit().unwrap();
            }

            let update = store.export_snapshot();
            drop(store);
            b.iter_batched(
                || {
                    let loro = LoroDoc::default();
                    loro.import(&update).unwrap();
                    loro
                },
                |loro| {
                    let text = loro.get_text("text");
                    let mut txn = loro.txn().unwrap();

                    for TextAction { pos, ins, del } in actions.iter() {
                        text.delete(&mut txn, *pos, *del).unwrap();
                        text.insert(&mut txn, *pos, ins).unwrap();
                    }
                },
                criterion::BatchSize::SmallInput,
            )
        });

        b.bench_function("B4 Obs", |b| {
            b.iter(|| {
                let loro = LoroDoc::default();
                let text = loro.get_text("text");
                loro.subscribe_deep(Arc::new(move |event| {
                    black_box(event);
                }));
                let mut txn = loro.txn().unwrap();
                for TextAction { pos, ins, del } in actions.iter() {
                    text.delete(&mut txn, *pos, *del).unwrap();
                    text.insert(&mut txn, *pos, ins).unwrap();
                }
            })
        });

        b.bench_function("B4 encode snapshot", |b| {
            let loro = LoroDoc::default();
            let text = loro.get_text("text");

            let mut n = 0;
            let mut txn = loro.txn().unwrap();
            for TextAction { pos, ins, del } in actions.iter() {
                if n == 10 {
                    n = 0;
                    drop(txn);
                    txn = loro.txn().unwrap();
                }
                n += 1;
                text.delete(&mut txn, *pos, *del).unwrap();
                text.insert(&mut txn, *pos, ins).unwrap();
            }
            txn.commit().unwrap();

            b.iter(|| {
                loro.export_snapshot();
            });
        });

        b.bench_function("B4 encode updates", |b| {
            let loro = LoroDoc::default();
            let text = loro.get_text("text");

            let mut n = 0;
            let mut txn = loro.txn().unwrap();
            for TextAction { pos, ins, del } in actions.iter() {
                if n == 10 {
                    n = 0;
                    drop(txn);
                    txn = loro.txn().unwrap();
                }
                n += 1;
                text.delete(&mut txn, *pos, *del).unwrap();
                text.insert(&mut txn, *pos, ins).unwrap();
            }
            txn.commit().unwrap();

            b.iter(|| {
                loro.export_from(&Default::default());
            });
        });

        b.bench_function("B4 decode snapshot", |b| {
            let loro = LoroDoc::default();
            let text = loro.get_text("text");
            let mut n = 0;
            let mut txn = loro.txn().unwrap();
            for TextAction { pos, ins, del } in actions.iter() {
                if n == 10 {
                    n = 0;
                    drop(txn);
                    txn = loro.txn().unwrap();
                }
                n += 1;
                text.delete(&mut txn, *pos, *del).unwrap();
                text.insert(&mut txn, *pos, ins).unwrap();
            }
            txn.commit().unwrap();

            let data = loro.export_snapshot();
            b.iter(|| {
                let l = LoroDoc::new();
                l.import(&data).unwrap();
            });
        });

        b.bench_function("B4 import updates", |b| {
            let loro = LoroDoc::default();
            let text = loro.get_text("text");

            let mut n = 0;
            let mut txn = loro.txn().unwrap();
            for TextAction { pos, ins, del } in actions.iter() {
                if n == 10 {
                    n = 0;
                    drop(txn);
                    txn = loro.txn().unwrap();
                }
                n += 1;
                text.delete(&mut txn, *pos, *del).unwrap();
                text.insert(&mut txn, *pos, ins).unwrap();
            }
            txn.commit().unwrap();

            let data = loro.export_from(&Default::default());
            b.iter(|| {
                let l = LoroDoc::new();
                l.import(&data).unwrap();
            });
        });

        // b.bench_function("B4 utf16", |b| {
        //     b.iter(|| {
        //         let loro = LoroDoc::new();
        //         let text = loro.get_text("text");
        //         let mut txn = loro.txn().unwrap();

        //         for TextAction { pos, ins, del } in actions.iter() {
        //             text.delete_utf16(&mut txn, *pos, *del).unwrap();
        //             text.insert_utf16(&mut txn, *pos, ins).unwrap();
        //         }
        //     })
        // });

        b.bench_function("B4_Per100_Txn", |b| {
            b.iter(|| {
                let loro = LoroDoc::default();
                let text = loro.get_text("text");
                let mut n = 0;
                let mut txn = loro.txn().unwrap();
                for TextAction { pos, ins, del } in actions.iter() {
                    if n == 100 {
                        n = 0;
                        drop(txn);
                        txn = loro.txn().unwrap();
                    }
                    n += 1;
                    text.delete(&mut txn, *pos, *del).unwrap();
                    text.insert(&mut txn, *pos, ins).unwrap();
                }
            })
        });

        b.bench_function("B4 One Op One Txn", |b| {
            b.iter(|| {
                let loro = LoroDoc::default();
                let text = loro.get_text("text");
                {
                    for TextAction { pos, ins, del } in actions.iter() {
                        let mut txn = loro.txn().unwrap();
                        text.delete(&mut txn, *pos, *del).unwrap();
                        text.insert(&mut txn, *pos, ins).unwrap();
                        txn.commit().unwrap();
                    }
                }
            })
        });

        b.bench_function("B4 One Op One Txn Obs", |b| {
            b.iter(|| {
                let loro = LoroDoc::default();
                let text = loro.get_text("text");
                loro.subscribe_deep(Arc::new(move |event| {
                    black_box(event);
                }));
                {
                    for TextAction { pos, ins, del } in actions.iter() {
                        let mut txn = loro.txn().unwrap();
                        text.delete(&mut txn, *pos, *del).unwrap();
                        text.insert(&mut txn, *pos, ins).unwrap();
                        txn.commit().unwrap();
                    }
                }
            })
        });

        b.bench_function("B4DirectSync", |b| {
            b.iter(|| {
                let loro = LoroDoc::default();
                let loro_b = LoroDoc::default();
                let text = loro.get_text("text");
                for TextAction { pos, ins, del } in actions.iter() {
                    {
                        let mut txn = loro.txn().unwrap();
                        text.delete(&mut txn, *pos, *del).unwrap();
                        text.insert(&mut txn, *pos, ins).unwrap();
                    }

                    loro_b
                        .import(&loro.export_from(&loro_b.oplog_vv()))
                        .unwrap();
                }
            })
        });

        drop(b);
        let mut b = c.benchmark_group("sync");
        b.bench_function("B4Parallel", |b| {
            b.iter(|| {
                let loro = LoroDoc::default();
                let loro_b = LoroDoc::default();
                let text = loro.get_text("text");
                let text2 = loro_b.get_text("text");
                let mut i = 0;
                for TextAction { pos, ins, del } in actions.iter() {
                    let pos = *pos;
                    let del = *del;
                    i += 1;
                    if i > 1000 {
                        break;
                    }

                    {
                        let mut txn = loro.txn().unwrap();
                        text.delete(&mut txn, pos, del).unwrap();
                        text.insert(&mut txn, pos, ins).unwrap();
                    }

                    {
                        let mut txn = loro_b.txn().unwrap();
                        text2.delete(&mut txn, pos, del).unwrap();
                        text2.insert(&mut txn, pos, ins).unwrap();
                    }
                    loro_b
                        .import(&loro.export_from(&loro_b.oplog_vv()))
                        .unwrap();
                    loro.import(&loro_b.export_from(&loro.oplog_vv())).unwrap();
                }
            })
        });

        let b = b.sample_size(10);
        b.bench_function("DecodeUpdates B4Parallel", |b| {
            let loro = LoroDoc::default();
            let loro_b = LoroDoc::default();
            let text = loro.get_text("text");
            let text2 = loro_b.get_text("text");
            for TextAction { pos, ins, del } in actions.iter() {
                let pos = *pos;
                let del = *del;
                {
                    let mut txn = loro.txn().unwrap();
                    text.delete(&mut txn, pos, del).unwrap();
                    text.insert(&mut txn, pos, ins).unwrap();
                }

                {
                    let mut txn = loro_b.txn().unwrap();
                    text2.delete(&mut txn, pos, del).unwrap();
                    text2.insert(&mut txn, pos, ins).unwrap();
                }
                loro_b
                    .import(&loro.export_from(&loro_b.oplog_vv()))
                    .unwrap();
                loro.import(&loro_b.export_from(&loro.oplog_vv())).unwrap();
            }
            let data = loro.export_from(&Default::default());
            b.iter(|| {
                let loro = LoroDoc::default();
                loro.import(&data).unwrap();
            })
        });
        b.bench_function("DecodeSnapshot B4Parallel", |b| {
            let loro = LoroDoc::default();
            let loro_b = LoroDoc::default();
            let text = loro.get_text("text");
            let text2 = loro_b.get_text("text");
            for TextAction { pos, ins, del } in actions.iter() {
                let pos = *pos;
                let del = *del;
                {
                    let mut txn = loro.txn().unwrap();
                    text.delete(&mut txn, pos, del).unwrap();
                    text.insert(&mut txn, pos, ins).unwrap();
                }

                {
                    let mut txn = loro_b.txn().unwrap();
                    text2.delete(&mut txn, pos, del).unwrap();
                    text2.insert(&mut txn, pos, ins).unwrap();
                }
                loro_b
                    .import(&loro.export_from(&loro_b.oplog_vv()))
                    .unwrap();
                loro.import(&loro_b.export_from(&loro.oplog_vv())).unwrap();
            }
            let data = loro.export_snapshot();
            b.iter(|| {
                let loro = LoroDoc::default();
                loro.import(&data).unwrap();
            })
        });
    }
}
pub fn dumb(_c: &mut Criterion) {}

#[cfg(feature = "test_utils")]
criterion_group!(benches, run::b4);
#[cfg(not(feature = "test_utils"))]
criterion_group!(benches, dumb);
criterion_main!(benches);
