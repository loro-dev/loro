use loro::{ExportMode, Frontiers, LoroDoc, ID};
use std::{hint::black_box, time::Instant};

/// Cold snapshot import, lazy historical checkout, and concurrent update imports.
/// Run in release, with one test thread, on both revisions; compare three repeats.
#[test]
#[ignore]
fn perf_history_lazy_load() {
    let source = LoroDoc::new();
    source.set_peer_id(1).unwrap();
    source.set_change_merge_interval(0);
    let text = source.get_text("text");
    for _ in 0..600 {
        text.insert(0, &"x".repeat(30)).unwrap();
        source.commit();
    }
    source.set_peer_id(2).unwrap();
    for _ in 0..300 {
        text.insert(0, &"y".repeat(30)).unwrap();
        source.commit();
    }
    source.get_map("map").insert("key", 1).unwrap();
    source.commit();
    let snapshot = source.export(ExportMode::Snapshot).unwrap();
    let remote = LoroDoc::new();
    remote.set_peer_id(3).unwrap();
    remote.get_text("text").insert(0, "concurrent").unwrap();
    let updates = remote.export(ExportMode::all_updates()).unwrap();
    let map_remote = LoroDoc::new();
    map_remote.set_peer_id(4).unwrap();
    map_remote.get_map("map").insert("key", 2).unwrap();
    let map_updates = map_remote.export(ExportMode::all_updates()).unwrap();
    for repeat in 1..=3 {
        let mut import = std::time::Duration::ZERO;
        let mut checkout = import;
        let mut concurrent = import;
        let mut map_import = import;
        for _ in 0..30 {
            let doc = LoroDoc::new();
            let start = Instant::now();
            doc.import(black_box(&snapshot)).unwrap();
            import += start.elapsed();
            let start = Instant::now();
            doc.checkout(&Frontiers::from(ID::new(1, 10))).unwrap();
            checkout += start.elapsed();
            black_box(doc.get_deep_value());
            let doc = LoroDoc::new();
            doc.import(&snapshot).unwrap();
            let start = Instant::now();
            doc.import(black_box(&updates)).unwrap();
            concurrent += start.elapsed();
            black_box(doc.get_deep_value());
            let doc = LoroDoc::new();
            doc.import(&snapshot).unwrap();
            let start = Instant::now();
            doc.import(black_box(&map_updates)).unwrap();
            map_import += start.elapsed();
            black_box(doc.get_deep_value());
        }
        println!(
            "repeat={repeat} snapshot_import_us={} cold_checkout_us={} concurrent_import_us={} map_import_us={}",
            import.as_micros(),
            checkout.as_micros(),
            concurrent.as_micros(),
            map_import.as_micros()
        );
    }
}
