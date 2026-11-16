//! Live heap around `reclaim_memory`, whose forced Stage 1 copies the string
//! arena (`StrArena::shrink_spare_capacity`). Run with
//! `cargo run -p loro --release --example arena_shrink_bench`.
use loro::{ExportMode, LoroDoc};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Relaxed);
        ALLOCATED.fetch_add(layout.size(), Relaxed);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        LIVE.fetch_add(size, Relaxed);
        LIVE.fetch_sub(layout.size(), Relaxed);
        ALLOCATED.fetch_add(size, Relaxed);
        System.realloc(ptr, layout, size)
    }
}

#[global_allocator]
static A: Counting = Counting;

fn measure(name: &str, doc: &LoroDoc) {
    let live = LIVE.load(Relaxed);
    let allocated = ALLOCATED.load(Relaxed);
    let _ = doc.reclaim_memory();
    let after = LIVE.load(Relaxed);
    let allocated_during = ALLOCATED.load(Relaxed) - allocated;
    println!(
        "{name},live_before={live},live_after={after},delta={},allocated_during_reclaim={allocated_during}",
        after as i64 - live as i64
    );
}

fn main() {
    // Local typing: text state slices point into the arena.
    let doc = LoroDoc::new();
    let text = doc.get_text("t");
    for i in 0..30_000 {
        text.insert(text.len_unicode(), &format!("w{i} ")).unwrap();
        if i % 100 == 0 {
            doc.commit();
        }
    }
    doc.commit();
    measure("local_text", &doc);

    // Local map strings.
    let doc2 = LoroDoc::new();
    let map = doc2.get_map("m");
    for i in 0..20_000 {
        map.insert(&format!("k{i}"), format!("value {i}")).unwrap();
    }
    doc2.commit();
    measure("local_map", &doc2);

    // Imported updates.
    let doc3 = LoroDoc::new();
    doc3.import(&doc.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    measure("imported_text", &doc3);
}
