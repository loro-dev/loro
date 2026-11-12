//! Run hostile inputs in capped child processes: an allocation regression must
//! fail the test without reserving tens of gigabytes in the test runner.
use loro::{ExportMode, LoroDoc, LoroError};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};

struct Capped;
static TRACK: AtomicBool = AtomicBool::new(false);
static LARGEST: AtomicUsize = AtomicUsize::new(0);
const CAP: usize = 1024 * 1024;

fn allowed(size: usize) -> bool {
    if TRACK.load(Relaxed) {
        LARGEST.fetch_max(size, Relaxed);
        size <= CAP
    } else {
        true
    }
}

unsafe impl GlobalAlloc for Capped {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !allowed(layout.size()) {
            return std::ptr::null_mut();
        }
        System.alloc(layout)
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !allowed(layout.size()) {
            return std::ptr::null_mut();
        }
        System.alloc_zeroed(layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if !allowed(size) {
            return std::ptr::null_mut();
        }
        System.realloc(ptr, layout, size)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }
}

#[global_allocator]
static ALLOC: Capped = Capped;

#[test]
fn import_header_counts_do_not_preallocate_from_untrusted_bytes() {
    const CASES: [&str; 6] = [
        "import_damaged",
        "meta_checked",
        "meta_unchecked",
        "batch_damaged",
        "import_crafted",
        "batch_crafted",
    ];
    let Ok(case) = std::env::var("LORO_HEADER_ALLOCATION_CASE") else {
        for case in CASES {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "import_header_counts_do_not_preallocate_from_untrusted_bytes",
                    "--nocapture",
                ])
                .env("LORO_HEADER_ALLOCATION_CASE", case)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{case}: {}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    };
    let source = LoroDoc::new();
    source.set_peer_id(1).unwrap();
    let text = source.get_text("text");
    text.insert(0, "hello").unwrap();
    source.commit();
    text.insert(5, " world").unwrap();
    source.commit();
    let valid = source.export(ExportMode::all_updates()).unwrap();
    assert_eq!(valid.len(), 94);
    let mut damaged = valid.clone();
    assert_eq!(&damaged[40..45], &[0; 5]);
    damaged[40..45].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x0f]);
    let mut crafted = damaged.clone();
    let checksum = xxhash_rust::xxh32::xxh32(&crafted[20..], u32::from_le_bytes(*b"LORO"));
    crafted[16..20].copy_from_slice(&checksum.to_le_bytes());

    let doc = LoroDoc::new();
    doc.get_text("local").insert(0, "before").unwrap();
    doc.commit();
    let before = doc.get_deep_value();
    let version = doc.oplog_vv();
    TRACK.store(true, Relaxed);
    let result = match case.as_str() {
        "import_damaged" => doc.import(&damaged).map(drop),
        "meta_checked" => LoroDoc::decode_import_blob_meta(&damaged, true).map(drop),
        "meta_unchecked" => LoroDoc::decode_import_blob_meta(&damaged, false).map(drop),
        "batch_damaged" => doc.import_batch(&[valid.clone(), damaged]).map(drop),
        "import_crafted" => doc.import(&crafted).map(drop),
        "batch_crafted" => doc.import_batch(&[valid.clone(), crafted]).map(drop),
        _ => panic!("unknown case"),
    };
    TRACK.store(false, Relaxed);
    assert!(
        matches!(
            result,
            Err(LoroError::DecodeError(_)
                | LoroError::DecodeDataCorruptionError
                | LoroError::DecodeChecksumMismatchError)
        ),
        "{result:?}"
    );
    assert!(LARGEST.load(Relaxed) <= CAP);
    assert!(!doc.is_detached());
    assert_eq!(doc.get_deep_value(), before);
    assert_eq!(doc.oplog_vv(), version);
    doc.get_text("local").insert(6, " after").unwrap();
    doc.commit();
    assert_eq!(doc.get_text("local").to_string(), "before after");
    doc.import(&valid).unwrap();
    assert_eq!(doc.get_text("text").to_string(), "hello world");
}

#[test]
fn highly_compressed_consecutive_commits_roundtrip_without_false_rejection() {
    let source = LoroDoc::new();
    source.set_peer_id(1).unwrap();
    let text = source.get_text("text");
    // 5,000 consecutive 1-op commits from one peer with identical timestamps,
    // sequential counters, and 0 external deps maximally compress RLE and
    // delta-of-delta columns across multiple blocks.
    for i in 0..5_000 {
        text.insert(i, "a").unwrap();
        source.commit_with(loro::CommitOptions::new().timestamp(1_700_000_000));
    }

    // Also interleave multi-peer dependencies so `other_dep_num` RLE/delta-of-delta
    // columns exercise repeated external peer dependencies.
    let peer2 = LoroDoc::new();
    peer2.set_peer_id(2).unwrap();
    let peer3 = LoroDoc::new();
    peer3.set_peer_id(3).unwrap();
    for round in 0..200 {
        peer2.get_map("m").insert("k", round).unwrap();
        peer2.commit_with(loro::CommitOptions::new().timestamp(1_700_000_000));
        peer3.get_map("m").insert("k", round).unwrap();
        peer3.commit_with(loro::CommitOptions::new().timestamp(1_700_000_000));
        let u2 = peer2
            .export(ExportMode::updates(&source.oplog_vv()))
            .unwrap();
        let u3 = peer3
            .export(ExportMode::updates(&source.oplog_vv()))
            .unwrap();
        source.import_batch(&[u2, u3]).unwrap();
        text.insert(5_000 + round as usize, "b").unwrap();
        source.commit_with(loro::CommitOptions::new().timestamp(1_700_000_000));
    }

    let expected = source.get_deep_value();
    let updates = source.export(ExportMode::all_updates()).unwrap();
    let snapshot = source.export(ExportMode::Snapshot).unwrap();
    let shallow = source
        .export(ExportMode::shallow_snapshot(&source.oplog_frontiers()))
        .unwrap();

    for (label, bytes) in [
        ("updates", &updates),
        ("snapshot", &snapshot),
        ("shallow", &shallow),
    ] {
        LoroDoc::decode_import_blob_meta(bytes, true).expect(label);
        LoroDoc::decode_import_blob_meta(bytes, false).expect(label);
        let dst = LoroDoc::new();
        dst.import(bytes).expect(label);
        assert_eq!(dst.get_deep_value(), expected, "{label}");
        let batch_dst = LoroDoc::new();
        batch_dst
            .import_batch(std::slice::from_ref(bytes))
            .expect(label);
        assert_eq!(batch_dst.get_deep_value(), expected, "{label}");
    }
}
