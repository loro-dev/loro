use criterion::{criterion_group, criterion_main, Criterion};

#[cfg(feature = "test_utils")]
mod run {
    use super::*;
    use bench_utils::TextAction;
    use loro_internal::refactor::loro::LoroApp;

    pub fn b4(c: &mut Criterion) {
        let actions = bench_utils::get_automerge_actions();
        let mut b = c.benchmark_group("refactored direct_apply");
        b.sample_size(10);
        b.bench_function("B4", |b| {
            b.iter(|| {
                let loro = LoroApp::default();
                let text = loro.get_text("text");
                let mut txn = loro.txn().unwrap();

                for TextAction { pos, ins, del } in actions.iter() {
                    text.delete(&mut txn, *pos, *del);
                    text.insert(&mut txn, *pos, ins);
                }
            })
        });

        b.bench_function("B4 utf16", |b| {
            b.iter(|| {
                let loro = LoroApp::new();
                let text = loro.get_text("text");
                let mut txn = loro.txn().unwrap();

                for TextAction { pos, ins, del } in actions.iter() {
                    text.delete_utf16(&mut txn, *pos, *del);
                    text.insert_utf16(&mut txn, *pos, ins);
                }
            })
        });

        b.bench_function("B4_Per100_Txn", |b| {
            b.iter(|| {
                let loro = LoroApp::default();
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
                    text.delete(&mut txn, *pos, *del);
                    text.insert(&mut txn, *pos, ins);
                }
            })
        });

        b.bench_function("B4 One Op One Txn", |b| {
            b.iter(|| {
                let loro = LoroApp::default();
                let text = loro.get_text("text");
                {
                    for TextAction { pos, ins, del } in actions.iter() {
                        let mut txn = loro.txn().unwrap();
                        text.delete(&mut txn, *pos, *del);
                        text.insert(&mut txn, *pos, ins);
                        txn.commit().unwrap();
                    }
                }
            })
        });

        b.bench_function("B4DirectSync", |b| {
            b.iter(|| {
                let loro = LoroApp::default();
                let loro_b = LoroApp::default();
                let text = loro.get_text("text");
                for TextAction { pos, ins, del } in actions.iter() {
                    {
                        let mut txn = loro.txn().unwrap();
                        text.delete(&mut txn, *pos, *del);
                        text.insert(&mut txn, *pos, ins);
                    }

                    loro_b
                        .import(&loro.export_from(&loro_b.vv_cloned()))
                        .unwrap();
                }
            })
        });

        drop(b);
        let mut b = c.benchmark_group("refactored-sync");
        b.bench_function("B4Parallel", |b| {
            b.iter(|| {
                let loro = LoroApp::default();
                let loro_b = LoroApp::default();
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
                        text.delete(&mut txn, pos, del);
                        text.insert(&mut txn, pos, ins);
                    }

                    {
                        let mut txn = loro_b.txn().unwrap();
                        text2.delete(&mut txn, pos, del);
                        text2.insert(&mut txn, pos, ins);
                    }
                    loro_b
                        .import(&loro.export_from(&loro_b.vv_cloned()))
                        .unwrap();
                    loro.import(&loro_b.export_from(&loro.vv_cloned())).unwrap();
                }
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
