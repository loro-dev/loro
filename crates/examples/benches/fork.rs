use criterion::{criterion_group, criterion_main, Criterion};
use loro::LoroDoc;

fn bench_fork(c: &mut Criterion) {
    {
        let mut b = c.benchmark_group("fork");
        b.bench_function("fork 1000 times with text edit at each fork", |b| {
            b.iter(|| {
                let mut doc = LoroDoc::new();
                for _ in 0..1000 {
                    let text = doc.get_text("text");
                    text.insert(0, "Hi").unwrap();
                    doc = doc.fork();
                }
            });
        });
    }
}

criterion_group!(benches, bench_fork);
criterion_main!(benches);
