use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

fn setup_text_operations_current(doc: &loro::LoroDoc, num_ops: usize) {
    let text = doc.get_text("text");
    for i in 0..num_ops {
        text.insert(text.len_unicode(), &format!("Operation {}\n", i))
            .unwrap();
        doc.commit();
    }
}

fn setup_text_operations_v159(doc: &loro_v159::LoroDoc, num_ops: usize) {
    let text = doc.get_text("text");
    for i in 0..num_ops {
        text.insert(text.len_unicode(), &format!("Operation {}\n", i))
            .unwrap();
        doc.commit();
    }
}

fn setup_list_operations_current(doc: &loro::LoroDoc, num_ops: usize) {
    let list = doc.get_list("list");
    for i in 0..num_ops {
        list.push(i as i64).unwrap();
        doc.commit();
    }
}

fn setup_list_operations_v159(doc: &loro_v159::LoroDoc, num_ops: usize) {
    let list = doc.get_list("list");
    for i in 0..num_ops {
        list.push(i as i64).unwrap();
        doc.commit();
    }
}

fn setup_map_operations_current(doc: &loro::LoroDoc, num_ops: usize) {
    let map = doc.get_map("map");
    for i in 0..num_ops {
        map.insert(&format!("key{}", i), i as i64).unwrap();
        doc.commit();
    }
}

fn setup_map_operations_v159(doc: &loro_v159::LoroDoc, num_ops: usize) {
    let map = doc.get_map("map");
    for i in 0..num_ops {
        map.insert(&format!("key{}", i), i as i64).unwrap();
        doc.commit();
    }
}

fn benchmark_text_undo_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("text_undo_comparison");

    for num_ops in [10, 50, 100, 200, 500].iter() {
        // Benchmark current optimized version
        group.bench_with_input(
            BenchmarkId::new("optimized", num_ops),
            num_ops,
            |b, &num_ops| {
                b.iter(|| {
                    let doc = loro::LoroDoc::new();
                    let mut undo = loro::UndoManager::new(&doc);
                    setup_text_operations_current(&doc, num_ops);

                    // Undo all operations
                    let mut count = 0;
                    while undo.undo().unwrap() {
                        count += 1;
                    }
                    black_box(count)
                });
            },
        );

        // Benchmark v1.5.9 unoptimized version
        group.bench_with_input(
            BenchmarkId::new("v1.5.9", num_ops),
            num_ops,
            |b, &num_ops| {
                b.iter(|| {
                    let doc = loro_v159::LoroDoc::new();
                    let mut undo = loro_v159::UndoManager::new(&doc);
                    setup_text_operations_v159(&doc, num_ops);

                    // Undo all operations
                    let mut count = 0;
                    while undo.undo().unwrap() {
                        count += 1;
                    }
                    black_box(count)
                });
            },
        );
    }

    group.finish();
}

fn benchmark_list_undo_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("list_undo_comparison");

    for num_ops in [10, 50, 100, 200, 500].iter() {
        // Benchmark current optimized version
        group.bench_with_input(
            BenchmarkId::new("optimized", num_ops),
            num_ops,
            |b, &num_ops| {
                b.iter(|| {
                    let doc = loro::LoroDoc::new();
                    let mut undo = loro::UndoManager::new(&doc);
                    setup_list_operations_current(&doc, num_ops);

                    // Undo all operations
                    let mut count = 0;
                    while undo.undo().unwrap() {
                        count += 1;
                    }
                    black_box(count)
                });
            },
        );

        // Benchmark v1.5.9 unoptimized version
        group.bench_with_input(
            BenchmarkId::new("v1.5.9", num_ops),
            num_ops,
            |b, &num_ops| {
                b.iter(|| {
                    let doc = loro_v159::LoroDoc::new();
                    let mut undo = loro_v159::UndoManager::new(&doc);
                    setup_list_operations_v159(&doc, num_ops);

                    // Undo all operations
                    let mut count = 0;
                    while undo.undo().unwrap() {
                        count += 1;
                    }
                    black_box(count)
                });
            },
        );
    }

    group.finish();
}

fn benchmark_map_undo_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("map_undo_comparison");

    for num_ops in [10, 50, 100, 200].iter() {
        // Benchmark current optimized version
        group.bench_with_input(
            BenchmarkId::new("optimized", num_ops),
            num_ops,
            |b, &num_ops| {
                b.iter(|| {
                    let doc = loro::LoroDoc::new();
                    let mut undo = loro::UndoManager::new(&doc);
                    setup_map_operations_current(&doc, num_ops);

                    // Undo all operations
                    let mut count = 0;
                    while undo.undo().unwrap() {
                        count += 1;
                    }
                    black_box(count)
                });
            },
        );

        // Benchmark v1.5.9 unoptimized version
        group.bench_with_input(
            BenchmarkId::new("v1.5.9", num_ops),
            num_ops,
            |b, &num_ops| {
                b.iter(|| {
                    let doc = loro_v159::LoroDoc::new();
                    let mut undo = loro_v159::UndoManager::new(&doc);
                    setup_map_operations_v159(&doc, num_ops);

                    // Undo all operations
                    let mut count = 0;
                    while undo.undo().unwrap() {
                        count += 1;
                    }
                    black_box(count)
                });
            },
        );
    }

    group.finish();
}

fn benchmark_large_content_undo(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_content_undo");

    // Test with different content sizes
    for (num_ops, content_size) in [(10, 1000), (50, 1000), (100, 100), (100, 1000)].iter() {
        let label = format!("{}ops_{}bytes", num_ops, content_size);

        // Optimized version
        group.bench_with_input(
            BenchmarkId::new("optimized", &label),
            &(num_ops, content_size),
            |b, &(num_ops, content_size)| {
                b.iter(|| {
                    let doc = loro::LoroDoc::new();
                    let mut undo = loro::UndoManager::new(&doc);
                    let text = doc.get_text("text");

                    let content = "x".repeat(*content_size);
                    for i in 0..*num_ops {
                        text.insert(text.len_unicode(), &format!("{}: {}\n", i, content))
                            .unwrap();
                        doc.commit();
                    }

                    // Undo all
                    while undo.undo().unwrap() {}
                });
            },
        );

        // v1.5.9
        group.bench_with_input(
            BenchmarkId::new("v1.5.9", &label),
            &(num_ops, content_size),
            |b, &(num_ops, content_size)| {
                b.iter(|| {
                    let doc = loro_v159::LoroDoc::new();
                    let mut undo = loro_v159::UndoManager::new(&doc);
                    let text = doc.get_text("text");

                    let content = "x".repeat(*content_size);
                    for i in 0..*num_ops {
                        text.insert(text.len_unicode(), &format!("{}: {}\n", i, content))
                            .unwrap();
                        doc.commit();
                    }

                    // Undo all
                    while undo.undo().unwrap() {}
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    benchmark_text_undo_comparison,
    benchmark_list_undo_comparison,
    benchmark_map_undo_comparison,
    benchmark_large_content_undo
);
criterion_main!(benches);
