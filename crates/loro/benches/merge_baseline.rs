use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use loro::internal::{
    LoroDoc as InternalLoroDoc, MapHandler, TextHandler, UndoItemMeta as InternalUndoItemMeta,
    UndoManager as InternalUndoManager,
};
use loro::{LoroDoc, LoroMap, LoroText, UndoItemMeta, UndoManager};
use std::hint::black_box;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn seed_heterogeneous_doc() -> LoroDoc {
    let doc = LoroDoc::new();
    let root = doc.get_list("root_list");
    root.insert(0, 1).unwrap();
    root.insert(1, true).unwrap();
    let nested = root.insert_container(2, LoroMap::new()).unwrap();
    nested.insert("title", "loro").unwrap();
    nested.insert("count", 3).unwrap();
    let map = doc.get_map("root_map");
    map.insert("flag", true).unwrap();
    map.insert("count", 7).unwrap();
    let text = map.insert_container("text", LoroText::new()).unwrap();
    text.insert(0, "baseline").unwrap();
    let text = nested.insert_container("text", LoroText::new()).unwrap();
    text.insert(0, "baseline").unwrap();
    doc
}

fn seed_internal_heterogeneous_doc() -> InternalLoroDoc {
    let doc = InternalLoroDoc::new_auto_commit();
    let root = doc.get_list("root_list");
    root.insert(0, 1).unwrap();
    root.insert(1, true).unwrap();
    let nested = root
        .insert_container(2, MapHandler::new_detached())
        .unwrap();
    nested.insert("title", "loro").unwrap();
    nested.insert("count", 3).unwrap();
    let map = doc.get_map("root_map");
    map.insert("flag", true).unwrap();
    map.insert("count", 7).unwrap();
    let text = map
        .insert_container("text", TextHandler::new_detached())
        .unwrap();
    text.insert_unicode(0, "baseline").unwrap();
    let text = nested
        .insert_container("text", TextHandler::new_detached())
        .unwrap();
    text.insert_unicode(0, "baseline").unwrap();
    doc
}

fn bench_active_subscriptions(c: &mut Criterion) {
    c.bench_function("merge baseline/public active subscriptions", |b| {
        b.iter_batched(
            || {
                let doc = LoroDoc::new();
                let list = doc.get_list("list");
                let sub = doc.subscribe_root(Arc::new(|event| {
                    black_box(event.events.len());
                }));
                (doc, list, sub)
            },
            |(doc, list, sub)| {
                black_box(&sub);
                list.insert(0, 1).unwrap();
                list.insert(1, 2).unwrap();
                doc.commit();
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_internal_active_subscriptions(c: &mut Criterion) {
    c.bench_function("merge baseline/internal active subscriptions", |b| {
        b.iter_batched(
            || {
                let doc = InternalLoroDoc::new_auto_commit();
                let list = doc.get_list("list");
                let sub = doc.subscribe_root(Arc::new(|event| {
                    black_box(event.events.len());
                }));
                (doc, list, sub)
            },
            |(doc, list, sub)| {
                black_box(&sub);
                list.insert(0, 1).unwrap();
                list.insert(1, 2).unwrap();
                doc.commit_then_renew();
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_heterogeneous_reads(c: &mut Criterion) {
    let doc = seed_heterogeneous_doc();
    let root = doc.get_list("root_list");
    let map = doc.get_map("root_map");
    c.bench_function("merge baseline/public heterogeneous reads", |b| {
        b.iter(|| {
            black_box(root.get(0));
            black_box(root.get(2));
            black_box(map.get("flag"));
            black_box(map.get("text"));
            black_box(doc.get_by_str_path("root_list/2/title"));
            black_box(doc.get_by_str_path("root_list/2/text"));
            black_box(doc.get_by_str_path("root_map/text"));
            let mut list_values = Vec::new();
            root.for_each(|value| list_values.push(value));
            black_box(list_values);
            let map_values: Vec<_> = map.values().collect();
            black_box(map_values);
        });
    });
}

fn bench_internal_heterogeneous_reads(c: &mut Criterion) {
    let doc = seed_internal_heterogeneous_doc();
    let root = doc.get_list("root_list");
    let map = doc.get_map("root_map");
    c.bench_function("merge baseline/internal heterogeneous reads", |b| {
        b.iter(|| {
            black_box(root.get_(0));
            black_box(root.get_(2));
            black_box(map.get_("flag"));
            black_box(map.get_("text"));
            black_box(doc.get_by_str_path("root_list/2/title"));
            black_box(doc.get_by_str_path("root_list/2/text"));
            black_box(doc.get_by_str_path("root_map/text"));
            let mut list_values = Vec::new();
            root.for_each(|value| list_values.push(value));
            black_box(list_values);
            let map_values: Vec<_> = map.values().collect();
            black_box(map_values);
        });
    });
}

fn bench_diff_apply_diff(c: &mut Criterion) {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "hello").unwrap();
    doc.commit();
    let start = doc.state_frontiers();
    text.insert(5, " world").unwrap();
    doc.commit();
    let diff = doc.diff(&start, &doc.state_frontiers()).unwrap();

    c.bench_function("merge baseline/public diff apply_diff", |b| {
        b.iter_batched(
            || (doc.fork_at(&start), diff.clone()),
            |(fork, diff)| {
                fork.apply_diff(diff).unwrap();
                black_box(fork.get_deep_value());
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_undo_callbacks(c: &mut Criterion) {
    c.bench_function("merge baseline/public undo callbacks", |b| {
        b.iter_batched(
            || {
                let doc = LoroDoc::new();
                let text = doc.get_text("text");
                let callback_hits = Arc::new(AtomicUsize::new(0));
                let callback_hits_clone = callback_hits.clone();
                let mut undo = UndoManager::new(&doc);
                undo.set_merge_interval(0);
                undo.set_on_push(Some(Box::new(move |_, _, event| {
                    callback_hits_clone.fetch_add(
                        event.map(|event| event.events.len()).unwrap_or_default(),
                        Ordering::Relaxed,
                    );
                    UndoItemMeta::new()
                })));
                (doc, text, undo, callback_hits)
            },
            |(doc, text, mut undo, callback_hits)| {
                black_box(&mut undo);
                text.insert(0, "hello").unwrap();
                doc.commit();
                black_box(callback_hits.load(Ordering::Relaxed));
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_internal_undo_callbacks(c: &mut Criterion) {
    c.bench_function("merge baseline/internal undo callbacks", |b| {
        b.iter_batched(
            || {
                let doc = InternalLoroDoc::new_auto_commit();
                let text = doc.get_text("text");
                let callback_hits = Arc::new(AtomicUsize::new(0));
                let callback_hits_clone = callback_hits.clone();
                let undo = InternalUndoManager::new(&doc);
                undo.set_merge_interval(0);
                undo.set_on_push(Some(Box::new(move |_, _, event| {
                    callback_hits_clone.fetch_add(
                        event.map(|event| event.events.len()).unwrap_or_default(),
                        Ordering::Relaxed,
                    );
                    InternalUndoItemMeta::new()
                })));
                (doc, text, undo, callback_hits)
            },
            |(doc, text, undo, callback_hits)| {
                black_box(&undo);
                text.insert_unicode(0, "hello").unwrap();
                doc.commit_then_renew();
                black_box(callback_hits.load(Ordering::Relaxed));
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_active_subscriptions,
    bench_internal_active_subscriptions,
    bench_heterogeneous_reads,
    bench_internal_heterogeneous_reads,
    bench_diff_apply_diff,
    bench_undo_callbacks,
    bench_internal_undo_callbacks
);
criterion_main!(benches);
