use loro::{bytes::Bytes, ExportMode, LoroDoc, LoroError};
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

#[global_allocator]
static ALLOCATOR: LargestAllocation = LargestAllocation;

// Keep these integration tests in their own binary so unrelated tests cannot affect the allocation counter.
#[test]
fn from_snapshot_rejects_invalid_input_without_copying_it() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    const INPUT_SIZE: usize = 1024 * 1024;
    let invalid_magic = vec![0; INPUT_SIZE];
    let mut invalid_checksum = vec![0; INPUT_SIZE];
    invalid_checksum[..4].copy_from_slice(b"loro");
    invalid_checksum[20..22].copy_from_slice(&3u16.to_be_bytes());
    let checksum = xxhash_rust::xxh32::xxh32(&invalid_checksum[20..], u32::from_le_bytes(*b"LORO"));
    invalid_checksum[16..20].copy_from_slice(&(checksum ^ 1).to_le_bytes());

    // A valid updates envelope is still not accepted by a snapshot constructor.
    let mut updates = invalid_checksum.clone();
    updates[20..22].copy_from_slice(&4u16.to_be_bytes());
    let checksum = xxhash_rust::xxh32::xxh32(&updates[20..], u32::from_le_bytes(*b"LORO"));
    updates[16..20].copy_from_slice(&checksum.to_le_bytes());

    let mut outdated_snapshot = invalid_checksum.clone();
    outdated_snapshot[20..22].copy_from_slice(&2u16.to_be_bytes());
    // Legacy modes use MD5 over the mode and body. This is the digest for
    // mode 2 followed by INPUT_SIZE - 22 zero bytes.
    outdated_snapshot[4..20].copy_from_slice(&[
        106, 183, 192, 204, 235, 242, 135, 185, 16, 52, 208, 223, 199, 32, 181, 200,
    ]);

    for (label, input) in [
        ("invalid magic", invalid_magic),
        ("invalid checksum", invalid_checksum),
        ("updates mode", updates),
        ("outdated snapshot mode", outdated_snapshot),
    ] {
        LARGEST.store(0, Relaxed);
        let result = LoroDoc::from_snapshot(&input);
        let largest = LARGEST.load(Relaxed);
        assert!(matches!(
            result,
            Err(LoroError::DecodeError(_)
                | LoroError::DecodeChecksumMismatchError
                | LoroError::ImportUnsupportedEncodingMode)
        ));
        assert!(
            largest < input.len(),
            "{label}: allocated {largest} bytes before rejecting {} bytes of input",
            input.len(),
        );
    }
}

#[test]
fn from_snapshot_bytes_does_not_copy_the_snapshot() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    const VALUES: u32 = 8;
    const VALUE_SIZE: usize = 128 * 1024;
    let doc = LoroDoc::new();
    let map = doc.get_map("images");
    for i in 0..VALUES {
        // Pseudo-random bytes so the store cannot shrink them by compression.
        let mut x = 0x9E37_79B9u32.wrapping_mul(i + 1);
        let value: Vec<u8> = (0..VALUE_SIZE)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect();
        map.insert(&i.to_string(), value).unwrap();
    }
    doc.commit();
    let snapshot = Bytes::from(doc.export(ExportMode::Snapshot).unwrap());
    let expected = doc.get_deep_value();
    drop(doc);

    LARGEST.store(0, Relaxed);
    let restored = LoroDoc::from_snapshot_bytes(snapshot.clone()).unwrap();
    let largest = LARGEST.load(Relaxed);
    assert!(
        largest < snapshot.len() / 2,
        "allocated {largest} bytes while importing a {}-byte snapshot",
        snapshot.len(),
    );
    assert_eq!(restored.get_deep_value(), expected);
}
