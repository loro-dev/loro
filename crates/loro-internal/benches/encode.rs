use criterion::{criterion_group, criterion_main, Criterion};
#[cfg(feature = "test_utils")]
mod sync {

    use super::*;
    use bench_utils::{get_automerge_actions, TextAction};
    use loro_internal::{encoding::ExportMode, LoroDoc};

    pub fn b4(c: &mut Criterion) {
        let actions = get_automerge_actions();
        let mut b = c.benchmark_group("encode_with_sync");
        b.sample_size(10);
        b.bench_function("update", |b| {
            b.iter(|| {
                let c1 = LoroDoc::new();
                c1.set_peer_id(1).unwrap();
                let c2 = LoroDoc::new();
                c1.set_peer_id(2).unwrap();
                let t1 = c1.get_text("text");
                let t2 = c2.get_text("text");
                for (i, action) in actions.iter().enumerate() {
                    if i > 2000 {
                        break;
                    }
                    let TextAction { pos, ins, del } = action;
                    if i % 2 == 0 {
                        let mut txn = c1.txn().unwrap();
                        t1.delete_with_txn(&mut txn, *pos, *del).unwrap();
                        t1.insert_with_txn(&mut txn, *pos, ins).unwrap();
                        txn.commit().unwrap();

                        let update = c1.export(ExportMode::updates(&c2.oplog_vv())).unwrap();
                        c2.import(&update).unwrap();
                    } else {
                        let mut txn = c2.txn().unwrap();
                        t2.delete_with_txn(&mut txn, *pos, *del).unwrap();
                        t2.insert_with_txn(&mut txn, *pos, ins).unwrap();
                        txn.commit().unwrap();
                        let update = c2.export(ExportMode::updates(&c1.oplog_vv())).unwrap();
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
    use loro_internal::{encoding::ExportMode, LoroDoc};

    pub fn b4(c: &mut Criterion) {
        let loro = LoroDoc::default();
        let mut ran = false;
        let mut ensure_ran = || {
            if !ran {
                let actions = bench_utils::get_automerge_actions();
                let text = loro.get_text("text");
                for TextAction { pos, ins, del } in actions.iter() {
                    let mut txn = loro.txn().unwrap();
                    text.delete_with_txn(&mut txn, *pos, *del).unwrap();
                    text.insert_with_txn(&mut txn, *pos, ins).unwrap();
                }
                ran = true;
            }
        };

        let mut b = c.benchmark_group("encode");
        b.sample_size(10);
        b.bench_function("B4_encode_updates", |b| {
            ensure_ran();
            b.iter(|| {
                let _ = loro.export(ExportMode::all_updates()).unwrap();
            })
        });
        b.bench_function("B4_decode_updates", |b| {
            ensure_ran();
            let buf = loro.export(ExportMode::all_updates()).unwrap();

            b.iter(|| {
                let store2 = LoroDoc::default();
                store2.import(&buf).unwrap();
            })
        });
        b.bench_function("B4_decode_updates detached mode", |b| {
            ensure_ran();
            let buf = loro.export(ExportMode::all_updates()).unwrap();

            b.iter(|| {
                let store2 = LoroDoc::default();
                store2.detach();
                store2.import(&buf).unwrap();
            })
        });
        b.bench_function("B4_encode_snapshot", |b| {
            ensure_ran();
            b.iter(|| {
                let _ = loro.export(ExportMode::Snapshot).unwrap();
            })
        });
        b.bench_function("B4_decode_snapshot", |b| {
            ensure_ran();
            let buf = loro.export(ExportMode::Snapshot).unwrap();
            b.iter(|| {
                let store2 = LoroDoc::default();
                store2.import(&buf).unwrap();
            })
        });

        b.bench_function("B4_encode_json_update", |b| {
            ensure_ran();
            b.iter(|| {
                let _ = loro.export_json_updates(&Default::default(), &loro.oplog_vv(), true);
            })
        });
        b.bench_function("B4_decode_json_update", |b| {
            ensure_ran();
            let json = loro.export_json_updates(&Default::default(), &loro.oplog_vv(), true);
            b.iter(|| {
                let store2 = LoroDoc::default();
                store2.import_json_updates(json.clone()).unwrap();
            })
        });
    }
}

mod import {
    use criterion::Criterion;
    use loro_internal::{encoding::ExportMode, LoroDoc};

    #[allow(dead_code)]
    pub fn causal_iter(c: &mut Criterion) {
        let mut b = c.benchmark_group("causal_iter");
        b.sample_size(10);
        b.bench_function("parallel_500", |b| {
            b.iter(|| {
                let c1 = LoroDoc::new();
                c1.set_peer_id(1).unwrap();
                let c2 = LoroDoc::new();
                c1.set_peer_id(2).unwrap();

                let text1 = c1.get_text("text");
                let text2 = c2.get_text("text");
                for _ in 0..500 {
                    text1
                        .insert_with_txn(&mut c1.txn().unwrap(), 0, "1")
                        .unwrap();
                    text2
                        .insert_with_txn(&mut c2.txn().unwrap(), 0, "2")
                        .unwrap();
                }

                let updates = c2.export(ExportMode::updates(&c1.oplog_vv())).unwrap();
                c1.import(&updates).unwrap()
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
