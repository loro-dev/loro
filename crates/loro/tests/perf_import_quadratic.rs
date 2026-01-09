use loro::{ExportMode, LoroDoc};
use std::time::Instant;

#[test]
#[ignore]
fn perf_import_insert_split_quadratic_e2e() {
    // Run with:
    // cargo test -p loro perf_import_insert_split_quadratic_e2e -- --ignored --nocapture
    //
    // You can scale it with:
    // LORO_PERF_FRAGMENTS=16384 cargo test -p loro perf_import_insert_split_quadratic_e2e -- --ignored --nocapture
    const CHUNK_LEN: usize = 256;
    const PEER_A: u64 = 1;
    const PEER_B: u64 = 2;
    const PEER_C: u64 = 3;

    let fragments: usize = std::env::var("LORO_PERF_FRAGMENTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8192);
    assert!(fragments > 1);

    let doc_len = CHUNK_LEN * fragments;
    let expected_fragment_updates = (fragments as u64) * ((fragments - 1) as u64) / 2;

    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(PEER_A).unwrap();
    let text_a = doc_a.get_text("t");
    let base = "a".repeat(doc_len);
    text_a.insert(0, &base).unwrap();
    doc_a.commit();
    let size_a = doc_a.with_oplog(|oplog| oplog.diagnose_size());
    println!("doc_a: atom_ops={}, ops={}", size_a.total_atom_ops, size_a.total_ops);
    let updates_a = doc_a.export(ExportMode::all_updates()).unwrap();

    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(PEER_B).unwrap();
    let text_b = doc_b.get_text("t");
    doc_b.import(&updates_a).unwrap();
    let size_b = doc_b.with_oplog(|oplog| oplog.diagnose_size());
    println!(
        "doc_b(after import a): atom_ops={}, ops={}",
        size_b.total_atom_ops, size_b.total_ops
    );
    let base_vv = doc_b.oplog_vv();

    for i in 0..(fragments - 1) {
        let pos = (i + 1) * CHUNK_LEN + i;
        text_b.insert(pos, "x").unwrap();
    }
    doc_b.commit();
    let size_b2 = doc_b.with_oplog(|oplog| oplog.diagnose_size());
    println!(
        "doc_b(after inserts): atom_ops={}, ops={}, changes={}",
        size_b2.total_atom_ops, size_b2.total_ops, size_b2.total_changes
    );
    let updates_b = doc_b.export(ExportMode::updates(&base_vv)).unwrap();
    assert!(!updates_b.is_empty());
    println!("updates_b: bytes={}", updates_b.len());

    let doc_c = LoroDoc::new();
    doc_c.set_peer_id(PEER_C).unwrap();
    let text_c = doc_c.get_text("t");
    doc_c.import(&updates_a).unwrap();

    // Isolate oplog decode/merge cost by importing in detached mode.
    let doc_d = LoroDoc::new();
    doc_d.set_peer_id(PEER_C + 1).unwrap();
    let text_d = doc_d.get_text("t");
    doc_d.import(&updates_a).unwrap();
    doc_d.detach();
    let start = Instant::now();
    doc_d.import(&updates_b).unwrap();
    let detached_elapsed = start.elapsed();
    let start = Instant::now();
    doc_d.checkout_to_latest();
    let attach_elapsed = start.elapsed();
    assert_eq!(text_d.len_unicode(), doc_len + (fragments - 1));
    println!(
        "perf_import_insert_split_quadratic_detached: detached_elapsed={:?}, attach_elapsed={:?}",
        detached_elapsed, attach_elapsed
    );

    let start = Instant::now();
    doc_c.import(&updates_b).unwrap();
    let elapsed = start.elapsed();

    assert_eq!(text_c.len_unicode(), doc_len + (fragments - 1));
    println!(
        "perf_import_insert_split_quadratic_e2e: doc_len={}, fragments={}, expected_fragment_updates={}, elapsed={:?}",
        doc_len, fragments, expected_fragment_updates, elapsed
    );
}
