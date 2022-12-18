use criterion::{criterion_group, criterion_main, Criterion};
#[cfg(feature = "test_utils")]
mod run {
    use super::*;
    use bench_utils::TextAction;
    use loro_core::container::registry::ContainerWrapper;
    use loro_core::LoroCore;
    use loro_core::VersionVector;

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
        b.bench_function("B4_encode_changes_no_compress", |b| {
            b.iter(|| {
                let _ = loro.encode_changes(&VersionVector::new(), false);
            })
        });
        b.bench_function("B4_decode_changes_no_compress", |b| {
            let buf = loro.encode_changes(&VersionVector::new(), false);
            let mut store2 = LoroCore::default();
            // store2.get_list("list").insert(&store2, 0, "lll").unwrap();
            b.iter(|| {
                store2.decode_changes(&buf);
            })
        });
        b.bench_function("B4_encode_snapshot_no_compress", |b| {
            b.iter(|| {
                let _ = loro.encode_snapshot(false);
            })
        });
        b.bench_function("B4_decode_snapshot_no_compress", |b| {
            let buf = loro.encode_snapshot(false);
            b.iter(|| {
                let _ = LoroCore::decode_snapshot(&buf, Default::default(), None);
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
