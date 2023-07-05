use criterion::{criterion_group, criterion_main, Criterion};
#[cfg(feature = "test_utils")]
mod sync {

    use super::*;
    use bench_utils::{get_automerge_actions, TextAction};
    use loro_internal::LoroCore;

    pub fn b4(c: &mut Criterion) {
        let actions = get_automerge_actions();
        let mut b = c.benchmark_group("encode_with_sync");
        b.sample_size(10);
        b.bench_function("update", |b| {
            b.iter(|| {
                let mut c1 = LoroCore::new(Default::default(), Some(0));
                let mut c2 = LoroCore::new(Default::default(), Some(1));
                let mut t1 = c1.get_text("text");
                let mut t2 = c2.get_text("text");
                for (i, action) in actions.iter().enumerate() {
                    if i > 2000 {
                        break;
                    }
                    let TextAction { pos, ins, del } = action;
                    if i % 2 == 0 {
                        t1.delete(&c1, *pos, *del).unwrap();
                        t1.insert(&c1, *pos, ins).unwrap();

                        let update = c1.encode_from(c2.vv_cloned());
                        c2.decode(&update).unwrap();
                    } else {
                        t2.delete(&c2, *pos, *del).unwrap();
                        t2.insert(&c2, *pos, ins).unwrap();
                        let update = c2.encode_from(c1.vv_cloned());
                        c1.decode(&update).unwrap();
                    }
                }
            })
        });
        b.bench_function("rle update", |b| {
            b.iter(|| {
                let mut c1 = LoroCore::new(Default::default(), Some(0));
                let mut c2 = LoroCore::new(Default::default(), Some(1));
                let mut t1 = c1.get_text("text");
                let mut t2 = c2.get_text("text");
                for (i, action) in actions.iter().enumerate() {
                    if i > 2000 {
                        break;
                    }
                    let TextAction { pos, ins, del } = action;
                    if i % 2 == 0 {
                        t1.delete(&c1, *pos, *del).unwrap();
                        t1.insert(&c1, *pos, ins).unwrap();
                        let update = c1.encode_from(c2.vv_cloned());
                        c2.decode(&update).unwrap();
                    } else {
                        t2.delete(&c2, *pos, *del).unwrap();
                        t2.insert(&c2, *pos, ins).unwrap();
                        let update = c2.encode_from(c1.vv_cloned());
                        c1.decode(&update).unwrap();
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
    use loro_internal::log_store::EncodeMode;
    use loro_internal::{LoroCore, Transact, VersionVector};

    pub fn b4(c: &mut Criterion) {
        let actions = bench_utils::get_automerge_actions();
        let mut loro = LoroCore::default();
        let mut text = loro.get_text("text");
        let txn = loro.transact();
        for TextAction { pos, ins, del } in actions.iter() {
            text.delete(&txn, *pos, *del).unwrap();
            text.insert(&txn, *pos, ins).unwrap();
        }
        drop(txn);

        let mut b = c.benchmark_group("encode");
        b.bench_function("B4_encode_updates", |b| {
            b.iter(|| {
                let _ = loro.encode_with_cfg(EncodeMode::Updates(VersionVector::new()));
            })
        });
        b.bench_function("B4_decode_updates", |b| {
            let buf = loro.encode_with_cfg(EncodeMode::Updates(VersionVector::new()));

            b.iter(|| {
                let mut store2 = LoroCore::default();
                store2.decode(&buf).unwrap();
            })
        });
        b.bench_function("B4_encode_rle_updates", |b| {
            b.iter(|| {
                let _ = loro.encode_with_cfg(EncodeMode::RleUpdates(VersionVector::new()));
            })
        });
        b.bench_function("B4_decode_rle_updates", |b| {
            let buf = loro.encode_with_cfg(EncodeMode::RleUpdates(VersionVector::new()));
            b.iter(|| {
                let mut store2 = LoroCore::default();
                store2.decode(&buf).unwrap();
            })
        });
        b.bench_function("B4_encode_snapshot", |b| {
            b.iter(|| {
                let _ = loro.encode_all();
            })
        });
        b.bench_function("B4_decode_snapshot", |b| {
            let buf = loro.encode_all();
            b.iter(|| {
                let mut store2 = LoroCore::default();
                store2.decode(&buf).unwrap();
            })
        });
    }
}

mod import {
    use criterion::Criterion;
    use loro_internal::{configure::Configure, LoroCore};

    pub fn causal_iter(c: &mut Criterion) {
        let mut b = c.benchmark_group("causal_iter");
        b.sample_size(10);
        b.bench_function("parallel_500", |b| {
            b.iter(|| {
                let mut c1 = LoroCore::new(
                    Configure {
                        ..Default::default()
                    },
                    Some(1),
                );
                let mut c2 = LoroCore::new(
                    Configure {
                        ..Default::default()
                    },
                    Some(2),
                );
                let mut text1 = c1.get_text("text");
                let mut text2 = c2.get_text("text");
                for _ in 0..500 {
                    text1.insert(&c1, 0, "1").unwrap();
                    text2.insert(&c2, 0, "2").unwrap();
                }

                c1.decode(&c2.encode_from(c1.vv_cloned())).unwrap();
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
