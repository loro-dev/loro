use std::{
    cell::{LazyCell, OnceCell},
    ops::Deref,
    time::Instant,
};

use bench_utils::TextAction;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use dev_utils::ByteSize;
use loro::LoroDoc;
use rand::Rng;

fn bench_text(c: &mut Criterion) {
    use bench_utils::TextAction;

    let actions = LazyCell::new(bench_utils::get_automerge_actions);
    let doc: OnceCell<LoroDoc> = OnceCell::new();
    let doc_snapshot: OnceCell<Vec<u8>> = OnceCell::new();
    let doc_x100_snapshot: OnceCell<Vec<u8>> = OnceCell::new();
    let mut g = c.benchmark_group("text");
    g.bench_function("B4 apply", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            let actions: &Vec<TextAction> = actions.deref();
            for _ in 0..iters {
                let loro = apply_text_actions(actions, 1);
                if doc.get().is_none() {
                    doc.set(loro).unwrap();
                }
            }

            start.elapsed()
        });
    });

    g.bench_function("B4 export fast snapshot (has cache)", |b| {
        b.iter_batched(
            || {
                if doc.get().is_none() {
                    let the_doc = apply_text_actions(&actions, 1);
                    doc.set(the_doc).unwrap();
                }
                let doc = doc.get().unwrap();
                doc
            },
            |doc| {
                doc.export(loro::ExportMode::Snapshot).unwrap();
            },
            criterion::BatchSize::SmallInput,
        )
    });

    g.bench_function("B4 export fast snapshot (no cache)", |b| {
        b.iter_batched(
            || apply_text_actions(&actions, 1),
            |doc| {
                doc.export(loro::ExportMode::Snapshot).unwrap();
            },
            criterion::BatchSize::SmallInput,
        )
    });

    g.bench_function("B4 load", |b| {
        b.iter_batched(
            || {
                if doc.get().is_none() {
                    let the_doc = apply_text_actions(&actions, 1);
                    doc.set(the_doc).unwrap();
                }
                if doc_snapshot.get().is_none() {
                    let doc = doc.get().unwrap();
                    let snapshot = doc.export(loro::ExportMode::Snapshot).unwrap();
                    println!("B4 fast_snapshot size: {:?}", ByteSize(snapshot.len()));
                    doc_snapshot.set(snapshot).unwrap();
                }
                doc_snapshot.get().unwrap()
            },
            |snapshot| {
                let doc = LoroDoc::new();
                doc.import(snapshot).unwrap();
            },
            criterion::BatchSize::LargeInput,
        )
    });

    g.bench_function("B4x100 load", |b| {
        b.iter_batched(
            || {
                if doc_x100_snapshot.get().is_none() {
                    let doc = apply_text_actions(&actions, 100);
                    let snapshot = doc.export(loro::ExportMode::Snapshot).unwrap();
                    println!("B4x100 fast_snapshot size: {:?}", ByteSize(snapshot.len()));
                    doc_x100_snapshot.set(snapshot).unwrap();
                }

                doc_x100_snapshot.get().unwrap()
            },
            |snapshot| {
                let doc = LoroDoc::new();
                doc.import(snapshot).unwrap();
            },
            criterion::BatchSize::LargeInput,
        )
    });

    g.bench_function("B4x100 load and get value", |b| {
        b.iter_batched(
            || {
                if doc_x100_snapshot.get().is_none() {
                    let doc = apply_text_actions(&actions, 100);
                    let snapshot = doc.export(loro::ExportMode::Snapshot).unwrap();
                    println!("B4x100 fast_snapshot size: {:?}", ByteSize(snapshot.len()));
                    doc_x100_snapshot.set(snapshot).unwrap();
                }

                doc_x100_snapshot.get().unwrap()
            },
            |snapshot| {
                let doc = LoroDoc::new();
                doc.import(snapshot).unwrap();
                let text = doc.get_text("text");
                black_box(text.to_string());
            },
            criterion::BatchSize::LargeInput,
        )
    });
}

fn bench_update_text(c: &mut Criterion) {
    let mut g = c.benchmark_group("update_text");
    g.bench_function("Update 1024x1024 total different text", |b| {
        b.iter_batched(
            || {
                let from = "a".repeat(1024);
                let to = "b".repeat(1024);
                (from, to)
            },
            |(from, to)| {
                let doc = LoroDoc::new();
                let text = doc.get_text("text");
                text.update(&from, Default::default()).unwrap();
                text.update(&to, Default::default()).unwrap();
                assert_eq!(&text.to_string(), &to);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    g.bench_function("Update 1024x1024 random diff", |b| {
        b.iter_batched(
            || {
                fn rand_str(len: usize, seed: u64) -> String {
                    let mut rng: rand::rngs::StdRng = rand::SeedableRng::seed_from_u64(seed);
                    let mut s = String::new();
                    for _ in 0..len {
                        s.push(rng.gen_range(b'a'..b'z') as char);
                    }
                    s
                }

                let from = rand_str(1024, 42);
                let mut to = from.clone();
                to.insert_str(0, &rand_str(100, 43));
                to.replace_range(100..200, &rand_str(100, 44)); // Make some differences
                to.replace_range(500..504, &rand_str(4, 45));
                to.replace_range(600..700, &rand_str(5, 46));
                (from, to)
            },
            |(from, to)| {
                let doc = LoroDoc::new();
                let text = doc.get_text("text");
                text.update(&from, Default::default()).unwrap();
                text.update(&to, Default::default()).unwrap();
                assert_eq!(&text.to_string(), &to);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    g.bench_function("Update 1024x1024 text with 16 inserted chars", |b| {
        b.iter_batched(
            || {
                let from = "a".repeat(1024);
                let mut to = from.clone();
                to.insert_str(504, "b".repeat(16).as_str());
                (from, to)
            },
            |(from, to)| {
                let doc = LoroDoc::new();
                let text = doc.get_text("text");
                text.update(&from, Default::default()).unwrap();
                text.update(&to, Default::default()).unwrap();
                assert_eq!(&text.to_string(), &to);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn apply_text_actions(actions: &[bench_utils::TextAction], n: usize) -> LoroDoc {
    let loro = LoroDoc::new();
    let text = loro.get_text("text");
    for _ in 0..n {
        for TextAction { del, ins, pos } in actions.iter() {
            text.delete(*pos, *del).unwrap();
            text.insert(*pos, ins).unwrap();
        }
    }
    loro
}

criterion_group!(benches, bench_text, bench_update_text);
criterion_main!(benches);
