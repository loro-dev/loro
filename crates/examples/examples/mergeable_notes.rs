//! Benchmark for the scenario:
//!     { notes: LoroMap<noteId, LoroMap<{ mergeable_text: LoroText }>> }
//!
//! Compares mergeable child containers versus regular child containers when the
//! document holds many notes, each containing a single text body. Reports
//! snapshot wire size, export/import wall-clock time, deep-value time, and
//! resident memory at each stage. Memory is measured with the
//! `dev_utils::get_mem_usage` global-allocator counter.
//!
//! To keep each measurement honest the doc under test is the only live doc when
//! the measurement is taken — earlier docs are dropped first.

use dev_utils::{get_mem_usage, ByteSize};
use loro::{ExportMode, LoroDoc, LoroMap, LoroText};
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::time::Instant;

const N: usize = 100_000;
const BYTES_PER_NOTE: usize = 100;
const SEED: u64 = 0xC0FFEE;

fn make_payload(rng: &mut StdRng) -> String {
    // Use printable ASCII so the text codec doesn't degenerate; the bytes are still
    // random per note.
    let mut s = String::with_capacity(BYTES_PER_NOTE);
    for _ in 0..BYTES_PER_NOTE {
        let b = rng.gen_range(33u8..127u8);
        s.push(b as char);
    }
    s
}

fn build_mergeable(n: usize) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let notes: LoroMap = doc.get_map("notes");
    let mut rng = StdRng::seed_from_u64(SEED);
    for i in 0..n {
        let note = notes.ensure_mergeable_map(&format!("n{i}")).unwrap();
        let text: LoroText = note.ensure_mergeable_text("body").unwrap();
        text.insert(0, &make_payload(&mut rng)).unwrap();
    }
    doc.commit();
    doc
}

fn build_regular(n: usize) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let notes: LoroMap = doc.get_map("notes");
    let mut rng = StdRng::seed_from_u64(SEED);
    for i in 0..n {
        let note = notes
            .insert_container(&format!("n{i}"), LoroMap::new())
            .unwrap();
        let text: LoroText = note.insert_container("body", LoroText::new()).unwrap();
        text.insert(0, &make_payload(&mut rng)).unwrap();
    }
    doc.commit();
    doc
}

struct Report {
    label: &'static str,
    build_time: std::time::Duration,
    doc_resident: ByteSize,
    snapshot_size: ByteSize,
    snapshot_zstd_size: ByteSize,
    export_time: std::time::Duration,
    import_time: std::time::Duration,
    imported_resident: ByteSize,
    deep_value_time: std::time::Duration,
    after_deep_value_resident: ByteSize,
    shallow_snapshot_size: ByteSize,
    shallow_export_time: std::time::Duration,
}

fn measure(label: &'static str, builder: fn(usize) -> LoroDoc) -> Report {
    // Baseline: measure RSS before we touch anything for this scenario.
    let base_mem = get_mem_usage();

    let build_start = Instant::now();
    let doc = builder(N);
    let build_time = build_start.elapsed();
    let doc_resident = get_mem_usage() - base_mem;

    let export_start = Instant::now();
    let snapshot = doc.export(ExportMode::Snapshot).unwrap();
    let export_time = export_start.elapsed();
    let snapshot_size = ByteSize(snapshot.len());

    let zstd_start = Instant::now();
    let compressed = zstd::encode_all(snapshot.as_slice(), 0).unwrap();
    let snapshot_zstd_size = ByteSize(compressed.len());
    drop(compressed);
    let _ = zstd_start;

    let shallow_start = Instant::now();
    let shallow = doc
        .export(ExportMode::shallow_snapshot(&doc.oplog_frontiers()))
        .unwrap();
    let shallow_export_time = shallow_start.elapsed();
    let shallow_snapshot_size = ByteSize(shallow.len());
    drop(shallow);

    // Drop the source doc so the imported doc is the only thing in memory.
    drop(doc);
    let pre_import = get_mem_usage();

    let new_doc = LoroDoc::new();
    let import_start = Instant::now();
    new_doc.import(&snapshot).unwrap();
    let import_time = import_start.elapsed();
    let imported_resident = get_mem_usage() - pre_import;

    let dv_start = Instant::now();
    let v = new_doc.get_deep_value();
    let deep_value_time = dv_start.elapsed();
    let after_deep_value_resident = get_mem_usage() - pre_import;
    drop(v);

    drop(new_doc);
    drop(snapshot);

    Report {
        label,
        build_time,
        doc_resident,
        snapshot_size,
        snapshot_zstd_size,
        export_time,
        import_time,
        imported_resident,
        deep_value_time,
        after_deep_value_resident,
        shallow_snapshot_size,
        shallow_export_time,
    }
}

fn print_report(r: &Report) {
    println!("--- {} ---", r.label);
    println!("  build (in-memory):         {:?}", r.build_time);
    println!("  doc resident after build:  {}", r.doc_resident);
    println!("  snapshot size:             {}", r.snapshot_size);
    println!("  snapshot size (zstd):      {}", r.snapshot_zstd_size);
    println!("  shallow snapshot size:     {}", r.shallow_snapshot_size);
    println!("  export snapshot:           {:?}", r.export_time);
    println!("  export shallow snapshot:   {:?}", r.shallow_export_time);
    println!("  import snapshot:           {:?}", r.import_time);
    println!("  resident after import:     {}", r.imported_resident);
    println!("  get_deep_value:            {:?}", r.deep_value_time);
    println!("  resident after deep_value: {}", r.after_deep_value_resident);
}

fn main() {
    println!(
        "Scenario: {{ notes: LoroMap<noteId, LoroMap<{{ body: LoroText }}>> }}, n={}, bytes/note={}",
        N, BYTES_PER_NOTE
    );

    let regular = measure("regular (insert_container LoroText)", build_regular);
    print_report(&regular);

    // Force allocations from the prior run to drop before measuring the next case.
    let mid_mem = get_mem_usage();
    println!("baseline between scenarios: {}", mid_mem);

    let mergeable = measure("mergeable (ensure_mergeable_text)", build_mergeable);
    print_report(&mergeable);

    println!("\n=== Delta ===");
    let pct = |a: usize, b: usize| -> f64 {
        if b == 0 {
            0.0
        } else {
            (a as f64 - b as f64) / b as f64 * 100.0
        }
    };
    println!(
        "snapshot size: regular={} mergeable={} delta={:+.1}%",
        regular.snapshot_size,
        mergeable.snapshot_size,
        pct(mergeable.snapshot_size.0, regular.snapshot_size.0)
    );
    println!(
        "snapshot zstd: regular={} mergeable={} delta={:+.1}%",
        regular.snapshot_zstd_size,
        mergeable.snapshot_zstd_size,
        pct(mergeable.snapshot_zstd_size.0, regular.snapshot_zstd_size.0)
    );
    println!(
        "shallow snap:  regular={} mergeable={} delta={:+.1}%",
        regular.shallow_snapshot_size,
        mergeable.shallow_snapshot_size,
        pct(
            mergeable.shallow_snapshot_size.0,
            regular.shallow_snapshot_size.0
        )
    );
    println!(
        "doc resident:  regular={} mergeable={} delta={:+.1}%",
        regular.doc_resident,
        mergeable.doc_resident,
        pct(mergeable.doc_resident.0, regular.doc_resident.0)
    );
    println!(
        "post-import resident: regular={} mergeable={} delta={:+.1}%",
        regular.imported_resident,
        mergeable.imported_resident,
        pct(mergeable.imported_resident.0, regular.imported_resident.0)
    );
    println!(
        "build time:    regular={:?} mergeable={:?}",
        regular.build_time, mergeable.build_time
    );
    println!(
        "export time:   regular={:?} mergeable={:?}",
        regular.export_time, mergeable.export_time
    );
    println!(
        "import time:   regular={:?} mergeable={:?}",
        regular.import_time, mergeable.import_time
    );
    println!(
        "deep_value:    regular={:?} mergeable={:?}",
        regular.deep_value_time, mergeable.deep_value_time
    );
}
