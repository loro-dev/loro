use criterion::{criterion_group, criterion_main, Criterion};
#[cfg(feature = "test_utils")]
mod sync {

    use super::*;
    use bench_utils::{get_automerge_actions, TextAction};
    use loro_internal::LoroDoc;

    pub fn b4(c: &mut Criterion) {
        let actions = get_automerge_actions();
        let mut b = c.benchmark_group("encode_with_sync");
        b.sample_size(10);
        b.bench_function("update", |b| {
            b.iter(|| {
                let c1 = LoroDoc::new();
                c1.set_peer_id(1);
                let c2 = LoroDoc::new();
                c1.set_peer_id(2);
                let t1 = c1.get_text("text");
                let t2 = c2.get_text("text");
                for (i, action) in actions.iter().enumerate() {
                    if i > 2000 {
                        break;
                    }
                    let TextAction { pos, ins, del } = action;
                    if i % 2 == 0 {
                        let mut txn = c1.txn().unwrap();
                        t1.delete(&mut txn, *pos, *del).unwrap();
                        t1.insert(&mut txn, *pos, ins).unwrap();
                        txn.commit().unwrap();

                        let update = c1.export_from(&c2.vv_cloned());
                        c2.import(&update).unwrap();
                    } else {
                        let mut txn = c2.txn().unwrap();
                        t2.delete(&mut txn, *pos, *del).unwrap();
                        t2.insert(&mut txn, *pos, ins).unwrap();
                        txn.commit().unwrap();
                        let update = c2.export_from(&c1.vv_cloned());
                        c1.import(&update).unwrap();
                    }
                }
            })
        });
    }
}
#[cfg(feature = "test_utils")]
mod run {
    use super::*;
    use bench_utils::TextAction;
    use loro_internal::LoroDoc;

    pub fn b4(c: &mut Criterion) {
        let loro = LoroDoc::default();
        let mut ran = false;
        let mut ensure_ran = || {
            if !ran {
                let actions = bench_utils::get_automerge_actions();
                let text = loro.get_text("text");
                let mut txn = loro.txn().unwrap();
                for TextAction { pos, ins, del } in actions.iter() {
                    text.delete(&mut txn, *pos, *del).unwrap();
                    text.insert(&mut txn, *pos, ins).unwrap();
                }
                drop(txn);
                ran = true;
            }
        };

        let mut b = c.benchmark_group("encode");
        b.bench_function("B4_encode_updates", |b| {
            ensure_ran();
            b.iter(|| {
                let _ = loro.export_from(&Default::default());
            })
        });
        b.bench_function("B4_decode_updates", |b| {
            ensure_ran();
            let buf = loro.export_from(&Default::default());

            b.iter(|| {
                let store2 = LoroDoc::default();
                store2.import(&buf).unwrap();
            })
        });
        b.bench_function("B4_encode_snapshot", |b| {
            ensure_ran();
            b.iter(|| {
                let _ = loro.export_snapshot();
            })
        });
        b.bench_function("B4_decode_snapshot", |b| {
            ensure_ran();
            let buf = loro.export_snapshot();
            b.iter(|| {
                let store2 = LoroDoc::default();
                store2.import(&buf).unwrap();
            })
        });
    }
}

mod import {
    use criterion::Criterion;
    use loro_internal::LoroDoc;

    #[allow(dead_code)]
    pub fn causal_iter(c: &mut Criterion) {
        let mut b = c.benchmark_group("causal_iter");
        b.sample_size(10);
        b.bench_function("parallel_500", |b| {
            b.iter(|| {
                let c1 = LoroDoc::new();
                c1.set_peer_id(1);
                let c2 = LoroDoc::new();
                c1.set_peer_id(2);

                let text1 = c1.get_text("text");
                let text2 = c2.get_text("text");
                for _ in 0..500 {
                    text1.insert(&mut c1.txn().unwrap(), 0, "1").unwrap();
                    text2.insert(&mut c2.txn().unwrap(), 0, "2").unwrap();
                }

                c1.import(&c2.export_from(&c1.vv_cloned())).unwrap();
            })
        });
    }
}

pub fn dumb(_c: &mut Criterion) {}

#[cfg(feature = "test_utils")]
criterion_group!(benches, run::b4, sync::b4, import::causal_iter);
#[cfg(not(feature = "test_utils"))]
criterion_group!(benches, dumb);
criterion_main!(benches);
