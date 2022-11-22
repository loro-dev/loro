use criterion::{criterion_group, criterion_main, Criterion};
const RAW_DATA: &[u8; 901823] = include_bytes!("automerge-paper.json.gz");

#[cfg(feature = "test_utils")]
mod run {
    use std::io::Read;

    use super::*;
    use arbitrary::Unstructured;
    use flate2::read::GzDecoder;
    use loro_core::container::registry::ContainerWrapper;
    use loro_core::fuzz::test_multi_sites;
    use loro_core::fuzz::Action;
    use loro_core::LoroCore;
    use rand::Rng;
    use rand::SeedableRng;
    use serde_json::Value;

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
        let mut d = GzDecoder::new(&RAW_DATA[..]);
        let mut s = String::new();
        d.read_to_string(&mut s).unwrap();
        let json: Value = serde_json::from_str(&s).unwrap();
        let txns = json.as_object().unwrap().get("txns");
        println!("{}", txns.unwrap().as_array().unwrap().len());
        let mut b = c.benchmark_group("direct_apply");
        b.bench_function("B4", |b| {
            b.iter(|| {
                let mut loro = LoroCore::default();
                let text = loro.get_text("text");
                text.with_container(|text| {
                    for txn in txns.unwrap().as_array().unwrap() {
                        let patches = txn
                            .as_object()
                            .unwrap()
                            .get("patches")
                            .unwrap()
                            .as_array()
                            .unwrap();
                        for patch in patches {
                            let pos = patch[0].as_u64().unwrap() as usize;
                            let del_here = patch[1].as_u64().unwrap() as usize;
                            let ins_content = patch[2].as_str().unwrap();
                            text.delete(&loro, pos, del_here);
                            text.insert(&loro, pos, ins_content);
                        }
                    }
                })
            })
        });

        b.sample_size(10);
        b.bench_function("B4DirectSync", |b| {
            b.iter(|| {
                let mut loro = LoroCore::default();
                let mut loro_b = LoroCore::default();
                for txn in txns.unwrap().as_array().unwrap() {
                    let text = loro.get_text("text");
                    text.with_container(|text| {
                        let patches = txn
                            .as_object()
                            .unwrap()
                            .get("patches")
                            .unwrap()
                            .as_array()
                            .unwrap();
                        for patch in patches {
                            let pos = patch[0].as_u64().unwrap() as usize;
                            let del_here = patch[1].as_u64().unwrap() as usize;
                            let ins_content = patch[2].as_str().unwrap();
                            text.delete(&loro, pos, del_here);
                            text.insert(&loro, pos, ins_content);
                        }
                    });

                    loro_b.import(loro.export(loro_b.vv()));
                }
            })
        });

        drop(b);
        let mut b = c.benchmark_group("sync");
        b.bench_function("B4Parallel", |b| {
            b.iter(|| {
                let mut loro = LoroCore::default();
                let mut loro_b = LoroCore::default();
                let mut i = 0;
                for txn in txns.unwrap().as_array().unwrap() {
                    i += 1;
                    if i > 1000 {
                        break;
                    }

                    let mut text = loro.get_text("text");
                    let patches = txn
                        .as_object()
                        .unwrap()
                        .get("patches")
                        .unwrap()
                        .as_array()
                        .unwrap();
                    for patch in patches {
                        let pos = patch[0].as_u64().unwrap() as usize;
                        let del_here = patch[1].as_u64().unwrap() as usize;
                        let ins_content = patch[2].as_str().unwrap();
                        text.delete(&loro, pos, del_here).unwrap();
                        text.insert(&loro, pos, ins_content).unwrap();
                    }

                    let mut text = loro_b.get_text("text");
                    for patch in patches {
                        let pos = patch[0].as_u64().unwrap() as usize;
                        let del_here = patch[1].as_u64().unwrap() as usize;
                        let ins_content = patch[2].as_str().unwrap();
                        text.delete(&loro_b, pos, del_here).unwrap();
                        text.insert(&loro_b, pos, ins_content).unwrap();
                    }
                    loro_b.import(loro.export(loro_b.vv()));
                    loro.import(loro_b.export(loro.vv()));
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
