//! Memory-measurement harness. Counting allocator + scripted stages.
use std::alloc::{GlobalAlloc, Layout};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};

#[cfg(feature = "jemalloc")]
pub type Inner = tikv_jemallocator::Jemalloc;
#[cfg(feature = "jemalloc")]
pub const INNER: Inner = tikv_jemallocator::Jemalloc;
#[cfg(feature = "mimalloc_alloc")]
pub type Inner = mimalloc::MiMalloc;
#[cfg(feature = "mimalloc_alloc")]
pub const INNER: Inner = mimalloc::MiMalloc;
#[cfg(not(any(feature = "jemalloc", feature = "mimalloc_alloc")))]
pub type Inner = std::alloc::System;
#[cfg(not(any(feature = "jemalloc", feature = "mimalloc_alloc")))]
pub const INNER: Inner = std::alloc::System;

pub struct Counting;
pub static LIVE: AtomicUsize = AtomicUsize::new(0);
pub static PEAK: AtomicUsize = AtomicUsize::new(0);
pub static LIVE_BLOCKS: AtomicUsize = AtomicUsize::new(0);
pub static ALLOCS: AtomicU64 = AtomicU64::new(0);
pub static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);

#[inline]
fn add(n: usize) {
    let v = LIVE.fetch_add(n, Relaxed) + n;
    PEAK.fetch_max(v, Relaxed);
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        add(l.size());
        LIVE_BLOCKS.fetch_add(1, Relaxed);
        ALLOCS.fetch_add(1, Relaxed);
        ALLOC_BYTES.fetch_add(l.size() as u64, Relaxed);
        INNER.alloc(l)
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        add(l.size());
        LIVE_BLOCKS.fetch_add(1, Relaxed);
        ALLOCS.fetch_add(1, Relaxed);
        ALLOC_BYTES.fetch_add(l.size() as u64, Relaxed);
        INNER.alloc_zeroed(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Relaxed);
        LIVE_BLOCKS.fetch_sub(1, Relaxed);
        INNER.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if n >= l.size() { add(n - l.size()); } else { LIVE.fetch_sub(l.size() - n, Relaxed); }
        ALLOCS.fetch_add(1, Relaxed);
        ALLOC_BYTES.fetch_add(n as u64, Relaxed);
        INNER.realloc(p, l, n)
    }
}

/// Global allocator lives here so both bins get it.
#[cfg(not(feature = "dhat_heap"))]
#[global_allocator]
static A: Counting = Counting;
#[cfg(feature = "dhat_heap")]
#[global_allocator]
static A: dhat::Alloc = dhat::Alloc;

pub fn proc_kb(field: &str) -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap();
    s.lines()
        .find(|l| l.starts_with(field))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|x| x.parse().ok())
        .unwrap_or(0)
}
pub fn mb(n: usize) -> f64 {
    n as f64 / 1048576.0
}
pub fn trim() {
    #[cfg(not(any(feature = "jemalloc", feature = "mimalloc_alloc")))]
    unsafe {
        libc::malloc_trim(0);
    }
}
