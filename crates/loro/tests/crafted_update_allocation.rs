//! Crafted updates whose run-length columns declare far more rows than their
//! bytes must be refused without expanding those runs (loro-dev/loro#1349).
use base64::Engine;
use loro::LoroDoc;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct LargestAllocation;

static LARGEST: AtomicUsize = AtomicUsize::new(0);
/// Serializes the tests in this file: they share the `LARGEST` counter.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

unsafe impl GlobalAlloc for LargestAllocation {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LARGEST.fetch_max(layout.size(), Relaxed);
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        LARGEST.fetch_max(size, Relaxed);
        System.realloc(ptr, layout, size)
    }
}

// Keep these tests in their own binary so unrelated tests cannot affect the counter.
#[global_allocator]
static ALLOCATOR: LargestAllocation = LargestAllocation;

/// No single allocation may come near the sizes the runs declare.
const MAX_ALLOCATION: usize = 1024 * 1024;

fn assert_import_is_refused_in_bounded_memory(label: &str, base64: &str) {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64)
        .unwrap();
    let doc = LoroDoc::new();

    LARGEST.store(0, Relaxed);
    let result = doc.import(&bytes);
    let largest = LARGEST.load(Relaxed);
    assert!(result.is_err(), "{label}: import was accepted: {result:?}");
    assert!(
        largest < MAX_ALLOCATION,
        "{label}: allocated {largest} bytes in one block for {} bytes of input",
        bytes.len()
    );

    LARGEST.store(0, Relaxed);
    let meta = LoroDoc::decode_import_blob_meta(&bytes, true);
    let largest = LARGEST.load(Relaxed);
    let _ = meta;
    assert!(
        largest < MAX_ALLOCATION,
        "{label}: decode_import_blob_meta allocated {largest} bytes in one block"
    );

    // The failed import leaves the document usable.
    doc.get_text("text").insert(0, "ok").unwrap();
    doc.commit();
    assert_eq!(doc.get_text("text").to_string(), "ok");
}

/// A 195-byte update whose delete-start-id column (delta run-length) declares a
/// run that the eager decoder expanded into a ~1 GiB `Vec`.
#[test]
fn delete_start_id_run_is_not_expanded() {
    assert_import_is_refused_in_bounded_memory(
        "delete start ids",
        "bG9ybwAAAAAAAAAAAAACAODPsigABKMBAA4ADgEQASoAAAAAAAAAAQEAAAAAAAUBAAABABUEBAEDAAYEAAAAAAQACAAADQACAAYXBHR5cGUGaW5saW5lBnN0cm9udwNkb2MJAQICAQADAQGAJgEECAUAAgAEAgYACQQAAwIBBgABEgluZwNkb2MJAQICAQADAQGAJgEECAUAAgAEAgYACfz//P0BBgABEgn/7/n2BwUMAAUGCAEBBwYBACYAAAABAQkh",
    );
}

/// A 207-byte update whose position arena (run-length `common_prefix_length`)
/// declares a run that the eager decoder expanded into a 16 GiB `Vec`.
#[test]
fn position_arena_run_is_not_expanded() {
    assert_import_is_refused_in_bounded_memory(
        "position arena",
        "bG9ybwAAAAAAAAAAAAAAACdVeW4ABKMBAA4ADgEQASoAAAAAAAAAAQEAAAAAAAUBAAABABUEBAEDAAZsb3JvAAAAVlVVVVVVVQEAVbM+kwA9np6engYAAAAAAAAAnp6enp6enp6enp6enp6enp6enp6enp6enp6enp6enp6enp6enp6enp6enp6enp6enp6enp6enp6enqMBAA4ADgEAAAAAAAAAABABKgAAAAAAAAABAQIAAAAABQEAAP0IAQAEBAACAAafjCGNhAUCAQEh",
    );
}
