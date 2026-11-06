//! A document keeps two tables open, so an idle table has to stay cheap when a process
//! holds thousands of small documents. Kept in its own binary: it counts every allocation.

use bytes::Bytes;
use loro_kv_store::mem_store::MemKvConfig;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct CountLiveBytes;

static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountLiveBytes {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Relaxed);
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        System.dealloc(ptr, layout);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        LIVE.fetch_add(size, Relaxed);
        LIVE.fetch_sub(layout.size(), Relaxed);
        System.realloc(ptr, layout, size)
    }
}

#[global_allocator]
static ALLOCATOR: CountLiveBytes = CountLiveBytes;

#[test]
fn a_one_block_table_does_not_reserve_a_full_size_block_cache() {
    let mut source = MemKvConfig::new().build();
    source.set(b"key", Bytes::from_static(b"value"));
    let table = source.export_all();
    drop(source);

    let before = LIVE.load(Relaxed);
    let mut stores = Vec::with_capacity(100);
    for _ in 0..100 {
        let mut store = MemKvConfig::new().build();
        store.import_all(table.clone()).unwrap();
        stores.push(store);
    }
    let per_store = (LIVE.load(Relaxed) - before) / 100;

    assert!(
        per_store < 2048,
        "one idle one-block table holds {per_store} bytes"
    );
}
