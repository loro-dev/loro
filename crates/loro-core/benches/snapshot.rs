use criterion::{black_box, criterion_group, criterion_main, Criterion};
const RAW_DATA: &[u8; 901823] = include_bytes!("automerge-paper.json.gz");

#[cfg(feature = "test_utils")]
mod run {
    use std::io::Read;
    use std::time::{Duration, Instant};

    use super::*;
    use flate2::read::GzDecoder;
    use loro_core::LoroCore;
    use serde_json::Value;

    pub fn b4(c: &mut Criterion) {
        let mut d = GzDecoder::new(&RAW_DATA[..]);
        let mut s = String::new();
        d.read_to_string(&mut s).unwrap();
        let json: Value = serde_json::from_str(&s).unwrap();
        let txns = json.as_object().unwrap().get("txns");

        let mut b = c.benchmark_group("encode");
        b.bench_function("B4_sync_by_snapshot", |b| {
            b.iter_custom(|iters| {
                let mut value = Duration::new(0, 0);
                for _ in 0..iters {
                    let mut loro = LoroCore::new(Default::default(), Some(0));
                    let mut loro_b = LoroCore::new(Default::default(), Some(1));

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
                        let start = Instant::now();
                        // black_box?
                        loro_b.decode_changes(&loro.encode_changes(&loro_b.vv(), false));
                        loro.decode_changes(&loro_b.encode_changes(&loro.vv(), false));
                        value += start.elapsed();
                    }
                }
                value
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
