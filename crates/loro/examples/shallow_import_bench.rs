//! Shallow-snapshot import: time, peak live heap, and the heap left after the
//! history is rewritten. Run with
//! `cargo run -p loro --release --example shallow_import_bench`.
use loro::{ExportMode, LoroDoc};
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

fn main() {
    let src = LoroDoc::new();
    src.set_peer_id(1).unwrap();
    let text = src.get_text("t");
    for i in 0..2_000 {
        src.get_map("m")
            .insert(&format!("k{i}"), "v".repeat(200))
            .unwrap();
    }
    src.commit();
    let middle = src.oplog_frontiers();
    for i in 0..20_000 {
        text.insert(text.len_unicode(), &format!("{i} ")).unwrap();
        if i % 10 == 0 {
            src.commit();
        }
    }
    src.commit();
    let snapshot = src.export(ExportMode::shallow_snapshot(&middle)).unwrap();
    drop(src);
    println!("round,bytes,import_ms,peak_extra_bytes,retained_after_rewrite_bytes");
    for round in 0..5 {
        let base = LIVE.load(Relaxed);
        PEAK.store(base, Relaxed);
        let start = Instant::now();
        let doc = LoroDoc::from_snapshot(&snapshot).unwrap();
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        let peak = PEAK.load(Relaxed) - base;
        doc.get_text("t").insert(0, "edit").unwrap();
        doc.commit();
        let _ = doc.reclaim_memory();
        let retained = LIVE.load(Relaxed) - base;
        println!("{round},{},{ms:.3},{peak},{retained}", snapshot.len());
        drop(doc);
    }
}
