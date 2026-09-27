//! Cost of re-exporting a shallow doc at its own (cached) shallow root.
//!
//! Re-export reuses the cached root state and filters it with the retention
//! rules (loro-dev/loro#1123). Pruning must not make a re-export cost a full
//! walk of the root state each time.
//!
//! Run with:
//! cargo test -p loro --test perf_shallow_reexport --release -- --ignored --nocapture --test-threads=1

use loro::{ExportMode, Frontiers, LoroDoc, LoroMap, TreeParentId};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

struct PeakAlloc;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for PeakAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ret = System.alloc(layout);
        if !ret.is_null() {
            let now = CURRENT.fetch_add(layout.size(), Relaxed) + layout.size();
            PEAK.fetch_max(now, Relaxed);
        }
        ret
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        CURRENT.fetch_sub(layout.size(), Relaxed);
    }
}

#[global_allocator]
static A: PeakAlloc = PeakAlloc;

/// Runs `f`, returning its result, wall time, and the peak heap growth above
/// the allocation level at entry, in MiB.
fn measure<T>(f: impl FnOnce() -> T) -> (T, Duration, f64) {
    let base = CURRENT.load(Relaxed);
    PEAK.store(base, Relaxed);
    let start = Instant::now();
    let out = f();
    let elapsed = start.elapsed();
    let peak = PEAK.load(Relaxed).saturating_sub(base);
    (out, elapsed, peak as f64 / (1024.0 * 1024.0))
}

/// `retained_ops` counts the ops kept after the root, including the root op.
fn filler(doc: &LoroDoc, retained_ops: usize) {
    let other = doc.get_map("other");
    for i in 0..retained_ops - 1 {
        other.insert(&format!("k{i}"), i as i64).unwrap();
        doc.commit();
    }
}

fn cut_with_filler(doc: &LoroDoc, retained_ops: usize) -> Frontiers {
    doc.commit();
    let cut = doc.oplog_frontiers();
    filler(doc, retained_ops);
    cut
}

fn many_maps(retained_ops: usize) -> (LoroDoc, Frontiers) {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let rows = doc.get_map("rows");
    for i in 0..100_000 {
        let row = rows
            .insert_container(&format!("r{i}"), LoroMap::new())
            .unwrap();
        row.insert("v", i as i64).unwrap();
    }
    let cut = cut_with_filler(&doc, retained_ops);
    (doc, cut)
}

fn big_text(retained_ops: usize) -> (LoroDoc, Frontiers) {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    let chunk = "0123456789abcdef".repeat(64 * 1024);
    for _ in 0..10 {
        text.insert(text.len_unicode(), &chunk).unwrap();
    }
    let cut = cut_with_filler(&doc, retained_ops);
    (doc, cut)
}

fn deep_tree(retained_ops: usize) -> (LoroDoc, Frontiers) {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let tree = doc.get_tree("tree");
    let mut parent = TreeParentId::Root;
    for i in 0..20_000 {
        if i % 1000 == 0 {
            parent = TreeParentId::Root;
        }
        parent = tree.create(parent).unwrap().into();
    }
    let cut = cut_with_filler(&doc, retained_ops);
    (doc, cut)
}

fn bench(name: &str, build: impl FnOnce() -> (LoroDoc, Frontiers)) {
    let (doc, cut) = build();
    let bytes = doc.export(ExportMode::shallow_snapshot(&cut)).unwrap();
    drop(doc);

    let (shallow, import_time, import_mib) = measure(|| {
        let shallow = LoroDoc::new();
        shallow.import(&bytes).unwrap();
        shallow
    });
    let root = shallow.shallow_since_frontiers();
    let export = || shallow.export(ExportMode::shallow_snapshot(&root)).unwrap();
    let (first, first_time, first_mib) = measure(export);
    const REPEAT: u32 = 10;
    let mut repeat_time = Duration::ZERO;
    let mut repeat_mib = 0f64;
    for _ in 0..REPEAT {
        let (again, t, mib) = measure(export);
        assert_eq!(again.len(), first.len());
        repeat_time += t;
        repeat_mib = repeat_mib.max(mib);
    }
    println!(
        "{name:<28} import {:>9.3} ms {:>7.2} MiB | first export {:>9.3} ms {:>7.2} MiB | repeated export {:>9.3} ms {:>7.2} MiB",
        import_time.as_secs_f64() * 1e3,
        import_mib,
        first_time.as_secs_f64() * 1e3,
        first_mib,
        (repeat_time / REPEAT).as_secs_f64() * 1e3,
        repeat_mib,
    );
}

#[test]
#[ignore]
fn perf_shallow_reexport_at_cached_root() {
    bench("100k maps / 256 retained", || many_maps(256));
    bench("100k maps / 257 retained", || many_maps(257));
    bench("10 MiB text / 256 retained", || big_text(256));
    bench("20k tree depth 1000 / 256", || deep_tree(256));
}
