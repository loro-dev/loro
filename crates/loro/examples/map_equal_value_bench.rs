//! Checkout/diff/import/undo cost when map keys and movable-list items move
//! between versions whose values are equal but whose winning ops differ
//! (loro-dev/loro#1124). Every map scenario uses 100k keys written 0 -> 1 -> 0.
//!
//! ```sh
//! cargo run -p loro --release --example map_equal_value_bench
//! ```
//!
//! Prints `scenario,median_ms` per line (medians of the timed iterations after
//! warm-up; setup, source export and fresh-doc imports are outside timers). To
//! compare two revisions, build this example on each and run the binaries
//! alternately several times on the same machine, then take the median per
//! scenario. `BENCH_FILTER=<substring>` runs only matching scenarios and
//! `BENCH_ITERS=<n>` overrides the number of timed iterations (e.g. to profile).

use loro::{ExportMode, Frontiers, LoroDoc, UndoManager, VersionVector};
use std::{hint::black_box, time::Instant};

const KEYS: usize = 100_000;
const LIST: usize = 20_000;
const WARMUP: usize = 2;
const DEFAULT_ITERS: usize = 9;

fn load(bytes: &[u8]) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.import(bytes).unwrap();
    doc
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// Runs `setup` then times `run` on its output, `WARMUP + ITERS` times.
fn bench<S>(name: &str, mut setup: impl FnMut() -> S, mut run: impl FnMut(S)) {
    if std::env::var("BENCH_FILTER").is_ok_and(|f| !name.contains(&f)) {
        return;
    }
    let iters = std::env::var("BENCH_ITERS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(DEFAULT_ITERS);
    let mut times = Vec::with_capacity(iters);
    for i in 0..WARMUP + iters {
        let input = setup();
        let start = Instant::now();
        run(input);
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        if i >= WARMUP {
            times.push(elapsed);
        }
    }
    println!("{name},{:.3}", median(times));
}

struct MapFixture {
    doc: LoroDoc,
    /// Every key is 0.
    a: Frontiers,
    /// Every key is 1.
    b: Frontiers,
    /// Every key is 0 again, written by newer ops.
    c: Frontiers,
    base: Vec<u8>,
    /// Updates from `a` to `b`, and from `a` to `c`.
    to_b: Vec<u8>,
    to_c: Vec<u8>,
}

fn map_fixture() -> MapFixture {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let map = doc.get_map("m");
    let keys: Vec<_> = (0..KEYS).map(|i| format!("key{i:06}")).collect();
    for k in &keys {
        map.insert(k, 0).unwrap();
    }
    doc.commit();
    let a = doc.oplog_frontiers();
    let a_vv: VersionVector = doc.oplog_vv();
    let base = doc.export(ExportMode::Snapshot).unwrap();
    for k in &keys {
        map.insert(k, 1).unwrap();
    }
    doc.commit();
    let b = doc.oplog_frontiers();
    let to_b = doc
        .fork_at(&b)
        .unwrap()
        .export(ExportMode::updates(&a_vv))
        .unwrap();
    for k in &keys {
        map.insert(k, 0).unwrap();
    }
    doc.commit();
    let c = doc.oplog_frontiers();
    let to_c = doc.export(ExportMode::updates(&a_vv)).unwrap();
    MapFixture {
        doc,
        a,
        b,
        c,
        base,
        to_b,
        to_c,
    }
}

fn concurrent_receiver(base: &[u8]) -> LoroDoc {
    let doc = load(base);
    doc.set_peer_id(2).unwrap();
    doc.get_map("m").insert("unrelated", 0).unwrap();
    doc.commit();
    black_box(doc.get_map("m").get_value());
    doc
}

fn map_benches() {
    let f = map_fixture();
    let d = &f.doc;
    for (label, target) in [("equal", &f.c), ("changed", &f.b)] {
        bench(
            &format!("map_checkout_roundtrip_{label}"),
            || (),
            |_| {
                d.checkout(&f.a).unwrap();
                d.checkout(target).unwrap();
            },
        );
        bench(
            &format!("map_diff_{label}"),
            || (),
            |_| {
                black_box(d.diff(&f.a, target).unwrap());
            },
        );
    }
    d.checkout_to_latest();
    for (label, updates) in [("equal", &f.to_c), ("changed", &f.to_b)] {
        bench(
            &format!("map_import_linear_{label}"),
            || {
                let doc = load(&f.base);
                black_box(doc.get_map("m").get_value());
                doc
            },
            |doc| doc.import(updates).map(|_| ()).unwrap(),
        );
        bench(
            &format!("map_import_concurrent_{label}"),
            || concurrent_receiver(&f.base),
            |doc| doc.import(updates).map(|_| ()).unwrap(),
        );
    }

    let keys: Vec<_> = (0..KEYS).map(|i| format!("key{i:06}")).collect();
    let undo_setup = || {
        let doc = load(&f.base);
        doc.set_peer_id(3).unwrap();
        let mut undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);
        let map = doc.get_map("m");
        for k in &keys {
            map.insert(k, 1).unwrap();
        }
        doc.commit();
        undo.record_new_checkpoint().unwrap();
        (doc, undo)
    };
    bench("map_undo", undo_setup, |(_doc, mut undo)| {
        black_box(undo.undo().unwrap());
    });
    bench(
        "map_redo",
        || {
            let (doc, mut undo) = undo_setup();
            undo.undo().unwrap();
            (doc, undo)
        },
        |(_doc, mut undo)| {
            black_box(undo.redo().unwrap());
        },
    );
}

fn movable_list_benches() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let list = doc.get_movable_list("l");
    for _ in 0..LIST {
        list.push(0).unwrap();
    }
    doc.commit();
    let a = doc.oplog_frontiers();
    let base = doc.export(ExportMode::Snapshot).unwrap();
    let a_vv = doc.oplog_vv();
    for i in 0..LIST {
        list.set(i, 1).unwrap();
    }
    doc.commit();
    let b = doc.oplog_frontiers();
    let to_b = doc.export(ExportMode::updates(&a_vv)).unwrap();
    for i in 0..LIST {
        list.set(i, 0).unwrap();
    }
    doc.commit();
    let c = doc.oplog_frontiers();

    bench(
        "movable_set_local",
        || {
            let doc = load(&base);
            let list = doc.get_movable_list("l");
            black_box(list.get_value());
            doc
        },
        |doc| {
            let list = doc.get_movable_list("l");
            for i in 0..LIST {
                list.set(i, 1).unwrap();
            }
            doc.commit();
        },
    );
    bench(
        "movable_import_set",
        || {
            let doc = load(&base);
            black_box(doc.get_movable_list("l").get_value());
            doc
        },
        |doc| doc.import(&to_b).map(|_| ()).unwrap(),
    );
    for (label, target) in [("equal", &c), ("changed", &b)] {
        bench(
            &format!("movable_checkout_roundtrip_{label}"),
            || (),
            |_| {
                doc.checkout(&a).unwrap();
                doc.checkout(target).unwrap();
            },
        );
    }
}

fn main() {
    map_benches();
    movable_list_benches();
}
