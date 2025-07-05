use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use loro_internal::{LoroDoc, UndoManager};
use std::time::Duration;

/// Benchmark undo performance with different document sizes
fn bench_undo_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("undo_performance");
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(5));
    
    // Test with different numbers of operations
    for num_ops in [10, 50, 100, 200, 500].iter() {
        // Benchmark with current version (optimized)
        group.bench_with_input(
            BenchmarkId::new("optimized", num_ops),
            num_ops,
            |b, &num_ops| {
                b.iter_batched(
                    || setup_doc_with_ops(num_ops),
                    |(doc, mut undo)| {
                        // Undo all operations
                        for _ in 0..num_ops {
                            black_box(undo.undo().unwrap());
                        }
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
        
        // Benchmark simulating old version (operations without precalculated diffs)
        group.bench_with_input(
            BenchmarkId::new("unoptimized", num_ops),
            num_ops,
            |b, &num_ops| {
                b.iter_batched(
                    || setup_doc_without_undo_manager(num_ops),
                    |(doc, mut undo)| {
                        // Undo all operations
                        for _ in 0..num_ops {
                            black_box(undo.undo().unwrap());
                        }
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    
    group.finish();
}

/// Benchmark consecutive undo/redo operations
fn bench_undo_redo_alternating(c: &mut Criterion) {
    let mut group = c.benchmark_group("undo_redo_alternating");
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(5));
    
    let num_ops = 100;
    let alternations = 50;
    
    group.bench_function("optimized", |b| {
        b.iter_batched(
            || {
                let (doc, mut undo) = setup_doc_with_ops(num_ops);
                // Undo half the operations first
                for _ in 0..num_ops/2 {
                    undo.undo().unwrap();
                }
                (doc, undo)
            },
            |(doc, mut undo)| {
                // Alternate between undo and redo
                for i in 0..alternations {
                    if i % 2 == 0 {
                        black_box(undo.undo().unwrap());
                    } else {
                        black_box(undo.redo().unwrap());
                    }
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });
    
    group.bench_function("unoptimized", |b| {
        b.iter_batched(
            || {
                let (doc, mut undo) = setup_doc_without_undo_manager(num_ops);
                // Undo half the operations first
                for _ in 0..num_ops/2 {
                    undo.undo().unwrap();
                }
                (doc, undo)
            },
            |(doc, mut undo)| {
                // Alternate between undo and redo
                for i in 0..alternations {
                    if i % 2 == 0 {
                        black_box(undo.undo().unwrap());
                    } else {
                        black_box(undo.redo().unwrap());
                    }
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });
    
    group.finish();
}

/// Benchmark undo with multiple containers
fn bench_undo_multiple_containers(c: &mut Criterion) {
    let mut group = c.benchmark_group("undo_multiple_containers");
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(5));
    
    let num_ops = 100;
    
    group.bench_function("optimized", |b| {
        b.iter_batched(
            || setup_doc_with_multiple_containers(num_ops),
            |(doc, mut undo)| {
                // Undo all operations
                for _ in 0..num_ops {
                    black_box(undo.undo().unwrap());
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });
    
    group.bench_function("unoptimized", |b| {
        b.iter_batched(
            || setup_doc_multiple_containers_without_undo(num_ops),
            |(doc, mut undo)| {
                // Undo all operations
                for _ in 0..num_ops {
                    black_box(undo.undo().unwrap());
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });
    
    group.finish();
}

/// Setup a document with operations tracked by UndoManager (optimized path)
fn setup_doc_with_ops(num_ops: usize) -> (LoroDoc, UndoManager) {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    let text = doc.get_text("text");
    
    for i in 0..num_ops {
        text.insert(0, &format!("Operation {} ", i)).unwrap();
        doc.commit_then_renew();
    }
    
    (doc, undo)
}

/// Setup a document with operations created before UndoManager (unoptimized path)
fn setup_doc_without_undo_manager(num_ops: usize) -> (LoroDoc, UndoManager) {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    
    // Create operations before UndoManager
    for i in 0..num_ops {
        text.insert(0, &format!("Operation {} ", i)).unwrap();
        doc.commit_then_renew();
    }
    
    // Create UndoManager after operations
    let undo = UndoManager::new(&doc);
    (doc, undo)
}

/// Setup a document with multiple containers (optimized)
fn setup_doc_with_multiple_containers(num_ops: usize) -> (LoroDoc, UndoManager) {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    
    let text = doc.get_text("text");
    let list = doc.get_list("list");
    let map = doc.get_map("map");
    
    for i in 0..num_ops {
        text.insert(0, &format!("Text {}", i)).unwrap();
        list.push(format!("Item {}", i)).unwrap();
        map.insert(&format!("key{}", i), i as i64).unwrap();
        doc.commit_then_renew();
    }
    
    (doc, undo)
}

/// Setup a document with multiple containers (unoptimized)
fn setup_doc_multiple_containers_without_undo(num_ops: usize) -> (LoroDoc, UndoManager) {
    let doc = LoroDoc::new();
    
    let text = doc.get_text("text");
    let list = doc.get_list("list");
    let map = doc.get_map("map");
    
    // Create operations before UndoManager
    for i in 0..num_ops {
        text.insert(0, &format!("Text {}", i)).unwrap();
        list.push(format!("Item {}", i)).unwrap();
        map.insert(&format!("key{}", i), i as i64).unwrap();
        doc.commit_then_renew();
    }
    
    // Create UndoManager after operations
    let undo = UndoManager::new(&doc);
    (doc, undo)
}

criterion_group!(
    benches,
    bench_undo_performance,
    bench_undo_redo_alternating,
    bench_undo_multiple_containers
);
criterion_main!(benches);