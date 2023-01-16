use criterion::{criterion_group, criterion_main, Criterion};
#[cfg(feature = "test_utils")]
mod sync {

    use super::*;
    use bench_utils::{get_automerge_actions, TextAction};
    use loro_core::container::registry::ContainerWrapper;
    use loro_core::LoroCore;

    pub fn b4(c: &mut Criterion) {
        let actions = get_automerge_actions();
        let mut b = c.benchmark_group("encode_with_sync");
        b.sample_size(10);
        b.bench_function("update", |b| {
            let mut c1 = LoroCore::new(Default::default(), Some(0));
            let mut c2 = LoroCore::new(Default::default(), Some(1));
            let t1 = c1.get_text("text");
            let t2 = c2.get_text("text");
            b.iter(|| {
                for (i, action) in actions.iter().enumerate() {
                    let TextAction { pos, ins, del } = action;
                    if i % 2 == 0 {
                        t1.with_container(|text| {
                            text.delete(&c1, *pos, *del);
                            text.insert(&c1, *pos, ins);
                        });
                        let update = c1.encode_from(c2.vv_cloned());
                        c2.decode(&update).unwrap();
                    } else {
                        t2.with_container(|text| {
                            text.delete(&c2, *pos, *del);
                            text.insert(&c2, *pos, ins);
                        });
                        let update = c2.encode_from(c1.vv_cloned());
                        c1.decode(&update).unwrap();
                    }
                }
            })
        });
        b.bench_function("rle update", |b| {
            let mut c1 = LoroCore::new(Default::default(), Some(0));
            let mut c2 = LoroCore::new(Default::default(), Some(1));
            let t1 = c1.get_text("text");
            let t2 = c2.get_text("text");
            b.iter(|| {
                for (i, action) in actions.iter().enumerate() {
                    let TextAction { pos, ins, del } = action;
                    if i % 2 == 0 {
                        t1.with_container(|text| {
                            text.delete(&c1, *pos, *del);
                            text.insert(&c1, *pos, ins);
                        });
                        let update = c1.encode_from(c2.vv_cloned());
                        c2.decode(&update).unwrap();
                    } else {
                        t2.with_container(|text| {
                            text.delete(&c2, *pos, *del);
                            text.insert(&c2, *pos, ins);
                        });
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
    use loro_core::container::registry::ContainerWrapper;
    use loro_core::log_store::{EncodeConfig, EncodeMode};
    use loro_core::{LoroCore, VersionVector};

    pub fn b4(c: &mut Criterion) {
        let actions = bench_utils::get_automerge_actions();
        let mut loro = LoroCore::default();
        let text = loro.get_text("text");
        text.with_container(|text| {
            for TextAction { pos, ins, del } in actions.iter() {
                text.delete(&loro, *pos, *del);
                text.insert(&loro, *pos, ins);
            }
        });

        let mut b = c.benchmark_group("encode");
        b.bench_function("B4_encode_updates", |b| {
            b.iter(|| {
                let _ = loro.encode_with_cfg(
                    EncodeConfig::new(EncodeMode::Updates(VersionVector::new())).without_compress(),
                );
            })
        });
        b.bench_function("B4_decode_updates", |b| {
            let buf = loro.encode_with_cfg(
                EncodeConfig::new(EncodeMode::Updates(VersionVector::new())).without_compress(),
            );
            let mut store2 = LoroCore::default();
            b.iter(|| {
                store2.decode(&buf).unwrap();
            })
        });
        b.bench_function("B4_encode_rle_updates", |b| {
            b.iter(|| {
                let _ = loro.encode_with_cfg(
                    EncodeConfig::new(EncodeMode::RleUpdates(VersionVector::new()))
                        .without_compress(),
                );
            })
        });
        b.bench_function("B4_decode_rle_updates", |b| {
            let buf = loro.encode_with_cfg(
                EncodeConfig::new(EncodeMode::RleUpdates(VersionVector::new())).without_compress(),
            );
            let mut store2 = LoroCore::default();
            b.iter(|| {
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
            let mut store2 = LoroCore::default();
            b.iter(|| {
                store2.decode(&buf).unwrap();
            })
        });
    }
}

mod import {
    use criterion::Criterion;
    use loro_core::{
        change::ChangeMergeCfg,
        configure::Configure,
        log_store::{EncodeConfig, EncodeMode},
        LoroCore,
    };

    pub fn causal_iter(c: &mut Criterion) {
        let mut b = c.benchmark_group("causal_iter");
        b.sample_size(10);
        b.bench_function("parallel_500", |b| {
            let mut c1 = LoroCore::new(
                Configure {
                    change: ChangeMergeCfg {
                        max_change_length: 0,
                        max_change_interval: 0,
                    },
                    ..Default::default()
                },
                Some(1),
            );
            let mut c2 = LoroCore::new(
                Configure {
                    change: ChangeMergeCfg {
                        max_change_length: 0,
                        max_change_interval: 0,
                    },
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
            b.iter(|| {
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
