//! Importing one big text insert made right after a concurrent change. The rope
//! reported the inserted leaf once per id-to-cursor fragment (256 ops), and each
//! report walked every fragment of the insert, so the import took time quadratic
//! in the insert's length (3M characters: ~50 s).
//!
//! Run the timing check with `cargo test --release -p loro --test big_insert_import
//! -- --ignored --nocapture`.

use loro::{ExportMode, LoroDoc};
use std::time::Instant;

fn text(n: usize) -> String {
    (0..n)
        .map(|i| (b'a' + ((i as u64 * 2_654_435_761) >> 7) as u8 % 26) as char)
        .collect()
}

/// Peer 1 inserts `n` characters at the start right after importing peer 2's
/// concurrent edit; returns both docs and peer 1's update for the insert.
fn big_insert(n: usize) -> (LoroDoc, LoroDoc, Vec<u8>) {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    a.get_text("text").insert(0, "Hello world").unwrap();
    a.commit();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    b.import(&a.export(ExportMode::Snapshot).unwrap()).unwrap();
    b.get_text("text").insert(0, "B ").unwrap();
    b.commit();
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let from = a.oplog_vv();
    a.get_text("text").insert(0, &text(n)).unwrap();
    a.commit();
    let update = a.export(ExportMode::updates(&from)).unwrap();
    (a, b, update)
}

#[test]
fn big_insert_after_concurrent_edit_imports_the_same_text() {
    let (a, b, update) = big_insert(20_000);
    b.import(&update).unwrap();
    assert_eq!(
        a.get_text("text").to_string(),
        b.get_text("text").to_string()
    );
    assert_eq!(
        a.get_text("text").len_unicode(),
        20_000 + "B Hello world".len()
    );
}

#[test]
#[ignore]
fn big_insert_import_is_not_quadratic() {
    let (a, b, update) = big_insert(1_000_000);
    let start = Instant::now();
    b.import(&update).unwrap();
    let elapsed = start.elapsed().as_secs_f64();
    println!("1M characters imported in {elapsed:.3}s");
    assert_eq!(
        a.get_text("text").to_string(),
        b.get_text("text").to_string()
    );
    // ~0.02 s linear on a laptop; ~5 s when quadratic.
    assert!(elapsed < 1.0, "1M-character import took {elapsed:.3}s");
}
