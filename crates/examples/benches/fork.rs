use criterion::{black_box, criterion_group, criterion_main, Criterion};
use loro::{LoroDoc, LoroText};

fn bench_fork(c: &mut Criterion) {
    let mut b = c.benchmark_group("fork");
    b.bench_function("fork with text edit at each fork", |b| {
        let snapshot = {
            let doc = LoroDoc::new();
            let map = doc.get_map("map");
            for i in 0..10000 {
                let text = map
                    .insert_container(&i.to_string(), LoroText::new())
                    .unwrap();
                text.insert(0, &i.to_string()).unwrap();
            }
            doc.export(loro::ExportMode::Snapshot).unwrap()
        };
        b.iter_with_setup(
            || {
                let doc = LoroDoc::new();
                doc.import(&snapshot).unwrap();
                doc.get_text("text").insert(0, "123").unwrap();
                doc
            },
            |doc| {
                black_box(doc.fork());
            },
        );
    });
}

criterion_group!(benches, bench_fork);
criterion_main!(benches);
