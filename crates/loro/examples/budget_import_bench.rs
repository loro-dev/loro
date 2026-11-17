//! Import cost with and without a document quota (which now computes the
//! inverse diff before changing the state). Run with
//! `cargo run -p loro --release --example budget_import_bench`.
use loro::{ExportMode, LoroDoc, MemoryBudget};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let now = LIVE.fetch_add(layout.size(), Relaxed) + layout.size();
        PEAK.fetch_max(now, Relaxed);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if size > layout.size() {
            let now = LIVE.fetch_add(size - layout.size(), Relaxed) + size - layout.size();
            PEAK.fetch_max(now, Relaxed);
        } else {
            LIVE.fetch_sub(layout.size() - size, Relaxed);
        }
        System.realloc(ptr, layout, size)
    }
}

#[global_allocator]
static A: Counting = Counting;

fn budget(on: bool) -> MemoryBudget {
    if on {
        MemoryBudget::new().with_max_doc_bytes(1 << 30)
    } else {
        MemoryBudget::new()
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn main() {
    // 20,000 text ops in 200 commits.
    let src = LoroDoc::new();
    src.set_peer_id(1).unwrap();
    let text = src.get_text("t");
    for i in 0..20_000 {
        let pos = (i * 7919) % (text.len_unicode() + 1);
        text.insert(pos, "x").unwrap();
        if i % 100 == 99 {
            src.commit();
        }
    }
    src.commit();
    let big = src.export(ExportMode::all_updates()).unwrap();

    // A 200k-character document receiving 300 small updates.
    let base_src = LoroDoc::new();
    base_src.set_peer_id(2).unwrap();
    let t = base_src.get_text("t");
    for i in 0..2_000 {
        t.insert(t.len_unicode(), &"y".repeat(100)).unwrap();
        if i % 50 == 0 {
            base_src.commit();
        }
    }
    base_src.commit();
    let base_snapshot = base_src.export(ExportMode::Snapshot).unwrap();
    let mut small_updates = Vec::new();
    for i in 0..300 {
        let vv = base_src.oplog_vv();
        t.insert((i * 997) % t.len_unicode(), "edit!").unwrap();
        base_src.commit();
        small_updates.push(base_src.export(ExportMode::updates(&vv)).unwrap());
    }

    println!("scenario,quota,median_ms,peak_extra_bytes");
    for quota in [false, true] {
        let mut times = Vec::new();
        let mut peaks = Vec::new();
        for _ in 0..7 {
            let doc = LoroDoc::new();
            doc.set_memory_budget(budget(quota));
            let base = LIVE.load(Relaxed);
            PEAK.store(base, Relaxed);
            let start = Instant::now();
            doc.import(&big).unwrap();
            times.push(start.elapsed().as_secs_f64() * 1000.0);
            peaks.push((PEAK.load(Relaxed) - base) as f64);
        }
        println!(
            "big_import,{quota},{:.2},{:.0}",
            median(times),
            median(peaks)
        );

        let mut times = Vec::new();
        let mut peaks = Vec::new();
        for _ in 0..5 {
            let doc = LoroDoc::new();
            doc.import(&base_snapshot).unwrap();
            let _ = doc.get_text("t").to_string();
            doc.set_memory_budget(budget(quota));
            let base = LIVE.load(Relaxed);
            PEAK.store(base, Relaxed);
            let start = Instant::now();
            for u in &small_updates {
                doc.import(u).unwrap();
            }
            times.push(start.elapsed().as_secs_f64() * 1000.0);
            peaks.push((PEAK.load(Relaxed) - base) as f64);
        }
        println!(
            "300_small_updates,{quota},{:.2},{:.0}",
            median(times),
            median(peaks)
        );
    }
}
