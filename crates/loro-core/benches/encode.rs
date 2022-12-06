use criterion::{criterion_group, criterion_main, Criterion};
const RAW_DATA: &[u8; 901823] = include_bytes!("automerge-paper.json.gz");

#[cfg(feature = "test_utils")]
mod run {
    use std::io::Read;

    use super::*;
    use flate2::read::GzDecoder;
    use loro_core::container::registry::ContainerWrapper;
    use loro_core::{LoroCore, VersionVector};
    use serde_json::Value;

    pub fn b4(c: &mut Criterion) {
        let mut d = GzDecoder::new(&RAW_DATA[..]);
        let mut s = String::new();
        d.read_to_string(&mut s).unwrap();
        let json: Value = serde_json::from_str(&s).unwrap();
        let txns = json.as_object().unwrap().get("txns");
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
        });
        let mut b = c.benchmark_group("encode");
        b.bench_function("B4_encode_changes_compress", |b| {
            b.iter(|| {
                let _ = loro.encode_changes(&VersionVector::new(), true);
            })
        });
        b.bench_function("B4_decode_changes_compress", |b| {
            let buf = loro.encode_changes(&VersionVector::new(), true);
            let mut store2 = LoroCore::default();
            b.iter(|| {
                store2.decode_changes(&buf);
            })
        });
        b.bench_function("B4_encode_changes_no_compress", |b| {
            b.iter(|| {
                let _ = loro.encode_changes(&VersionVector::new(), false);
            })
        });
        b.bench_function("B4_decode_changes_no_compress", |b| {
            let buf = loro.encode_changes(&VersionVector::new(), false);
            let mut store2 = LoroCore::default();
            b.iter(|| {
                store2.decode_changes(&buf);
            })
        });
        b.bench_function("B4_decode_changes_no_compress_with_apply", |b| {
            let buf = loro.encode_changes(&VersionVector::new(), false);
            let mut store2 = LoroCore::default();
            store2.get_list("list").insert(&store2, 0, "lll").unwrap();
            b.iter(|| {
                store2.decode_changes(&buf);
            })
        });
        b.bench_function("B4_export_snapshot_compress", |b| {
            b.iter(|| {
                let _ = loro.export_entire_snapshot(true);
            })
        });
        b.bench_function("B4_import_snapshot_compress", |b| {
            let buf = loro.export_entire_snapshot(true);
            b.iter(|| {
                let _ = LoroCore::import_entire_snapshot(&buf, Default::default(), None);
            })
        });
        b.bench_function("B4_export_snapshot_no_compress", |b| {
            b.iter(|| {
                let _ = loro.export_entire_snapshot(false);
            })
        });
        b.bench_function("B4_import_snapshot_no_compress", |b| {
            let buf = loro.export_entire_snapshot(false);
            b.iter(|| {
                let _ = LoroCore::import_entire_snapshot(&buf, Default::default(), None);
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
