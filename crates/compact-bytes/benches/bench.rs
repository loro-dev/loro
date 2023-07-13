use compact_bytes::CompactBytes;
use criterion::{black_box, criterion_group, criterion_main, Criterion};

pub fn entry(c: &mut Criterion) {
    let data = include_str!("./permuted.mht");
    let data_x4 = data.repeat(4);
    c.bench_function("compact-bytes", |b| {
        b.iter(|| {
            let mut bytes = CompactBytes::new();
            bytes.alloc_advance(black_box(data.as_bytes()));
        });
    });
    c.bench_function("compact-bytes x4", |b| {
        b.iter(|| {
            let mut bytes = CompactBytes::new();
            bytes.alloc_advance(black_box(data_x4.as_bytes()));
        });
    });
}

criterion_group!(benches, entry);
criterion_main!(benches);
