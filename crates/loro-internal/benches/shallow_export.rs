use criterion::{criterion_group, criterion_main, Criterion};
use loro_internal::{encoding::ExportMode, version::Frontiers, LoroDoc};
use std::hint::black_box;

/// Build a doc shaped like a real workspace export: a root list of
/// `OUTER_DOCS` maps, each with a nested list of `INNER_ITEMS` maps, each
/// holding `TEXTS_PER_ITEM` short text containers. Every text is written with
/// `OPS_PER_TEXT` single-character inserts to simulate streaming edits.
///
/// With the defaults this produces ~66k containers and ~720k ops.
fn build_structured_doc() -> (LoroDoc, Frontiers) {
    const OUTER_DOCS: usize = 200;
    const INNER_ITEMS: usize = 30;
    const TEXTS_PER_ITEM: usize = 10;
    const OPS_PER_TEXT: usize = 12;

    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let root = doc.get_list("docs");
    let mut mid = None;
    for i in 0..OUTER_DOCS {
        let map = root
            .insert_container(i, loro_internal::handler::MapHandler::new_detached())
            .unwrap();
        let items = map
            .insert_container("items", loro_internal::handler::ListHandler::new_detached())
            .unwrap();
        for j in 0..INNER_ITEMS {
            let item = items
                .insert_container(j, loro_internal::handler::MapHandler::new_detached())
                .unwrap();
            for k in 0..TEXTS_PER_ITEM {
                let text = item
                    .insert_container(
                        &format!("t{k}"),
                        loro_internal::handler::TextHandler::new_detached(),
                    )
                    .unwrap();
                for n in 0..OPS_PER_TEXT {
                    text.insert(n, "x", loro_internal::cursor::PosType::Unicode)
                        .unwrap();
                }
            }
        }
        if i == OUTER_DOCS / 2 {
            mid = Some(doc.oplog_frontiers());
        }
    }
    (doc, mid.unwrap())
}

fn shallow_export(c: &mut Criterion) {
    let (doc, mid_frontiers) = build_structured_doc();
    let mut g = c.benchmark_group("shallow_export");
    g.sample_size(10);
    g.bench_function("full_snapshot", |b| {
        b.iter(|| black_box(doc.export(ExportMode::Snapshot).unwrap()))
    });
    g.bench_function("shallow_snapshot", |b| {
        b.iter(|| {
            black_box(
                doc.export(ExportMode::shallow_snapshot(&mid_frontiers))
                    .unwrap(),
            )
        })
    });
    g.finish();
}

/// A document imported from a snapshot and never read stays lazy: exporting a
/// shallow snapshot at the latest version must not materialize the whole
/// state. Each iteration exports from a freshly imported doc, so this measures
/// the cold path (setup time is excluded).
fn shallow_export_lazy(c: &mut Criterion) {
    let (doc, _) = build_structured_doc();
    let full = doc.export(ExportMode::Snapshot).unwrap();
    let latest = doc.oplog_frontiers();
    let mut g = c.benchmark_group("shallow_export_lazy");
    g.sample_size(10);
    g.bench_function("at_latest", |b| {
        b.iter_batched(
            || {
                let lazy = LoroDoc::new();
                lazy.import(&full).unwrap();
                lazy
            },
            |lazy| black_box(lazy.export(ExportMode::shallow_snapshot(&latest)).unwrap()),
            criterion::BatchSize::LargeInput,
        )
    });
    g.finish();
}

criterion_group!(benches, shallow_export, shallow_export_lazy);
criterion_main!(benches);
