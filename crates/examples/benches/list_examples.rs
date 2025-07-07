use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use examples::list::random_insert;
use loro::LoroDoc;

fn bench_list(c: &mut Criterion) {
    {
        let mut b = c.benchmark_group("list insert");
        b.throughput(Throughput::Bytes(8 * 10_000));
        b.bench_function("10K", |b| {
            b.iter(|| {
                let doc = LoroDoc::new();
                let mut list = doc.get_list("list");
                random_insert(&mut list, 10_000, 100);
            });
        });
    }

    {
        let mut b = c.benchmark_group("movable list insert");
        b.throughput(Throughput::Bytes(8 * 10_000));
        b.bench_function("10K", |b| {
            b.iter(|| {
                let doc = LoroDoc::new();
                let mut list = doc.get_movable_list("list");
                random_insert(&mut list, 10_000, 100);
            });
        });
    }
}

criterion_group!(benches, bench_list);
criterion_main!(benches);
