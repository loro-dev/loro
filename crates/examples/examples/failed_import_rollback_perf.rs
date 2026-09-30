//! Ad-hoc perf probe for import rollback (loro-dev/loro#1164): successful imports, which the
//! rollback must not slow down, and the cost of a failed one.
//!
//! Run on two revisions and compare. Not part of CI.
use loro::{ExportMode, LoroDoc, LoroMap};
use std::time::{Duration, Instant};

fn best(rounds: usize, mut f: impl FnMut() -> Duration) -> Duration {
    (0..rounds).map(|_| f()).min().unwrap()
}

fn edit(doc: &LoroDoc, i: usize) {
    let text = doc.get_text("text");
    let len = text.len_unicode();
    text.insert((i * 7) % (len + 1), "hello ").unwrap();
    doc.get_list("list").push(i as i64).unwrap();
    let child = doc
        .get_map("map")
        .insert_container(&format!("k{}", i % 500), LoroMap::new())
        .unwrap();
    child.insert("v", i as i64).unwrap();
    doc.commit();
}

fn load(snapshot: &[u8]) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.import(snapshot).unwrap();
    let _ = doc.get_deep_value();
    doc
}

fn main() {
    let n: usize = std::env::var("N")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(5_000);
    let rounds = 5;

    // Small updates imported one by one.
    let src = LoroDoc::new();
    src.set_peer_id(1).unwrap();
    let mut updates = Vec::with_capacity(n);
    for i in 0..n {
        let vv = src.oplog_vv();
        edit(&src, i);
        updates.push(src.export(ExportMode::updates(&vv)).unwrap());
    }
    let small = best(rounds, || {
        let doc = LoroDoc::new();
        let start = Instant::now();
        for u in &updates {
            doc.import(u).unwrap();
        }
        start.elapsed()
    });

    // A large update into a doc that has state.
    let snapshot = src.export(ExportMode::Snapshot).unwrap();
    let base_vv = src.oplog_vv();
    let other = load(&snapshot);
    other.set_peer_id(2).unwrap();
    for i in 0..n {
        edit(&other, i);
    }
    let large_update = other.export(ExportMode::updates(&base_vv)).unwrap();
    let large = best(rounds, || {
        let doc = load(&snapshot);
        let start = Instant::now();
        doc.import(&large_update).unwrap();
        start.elapsed()
    });

    // A failed import: the large update plus a list insert forged out of bounds.
    other.get_list("list").insert(0, -1).unwrap();
    other.commit();
    let mut json =
        serde_json::to_value(other.export_json_updates(&base_vv, &other.oplog_vv())).unwrap();
    let last = json["changes"].as_array_mut().unwrap().last_mut().unwrap()["ops"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    last["content"]["pos"] = 1_000_000_000u64.into();
    let carrier = load(&snapshot);
    carrier.detach();
    carrier
        .import_json_updates(serde_json::to_string(&json).unwrap())
        .unwrap();
    let bad = carrier.export(ExportMode::updates(&base_vv)).unwrap();
    let doc = load(&snapshot);
    let failed = best(rounds, || {
        let start = Instant::now();
        assert!(doc.import(&bad).is_err());
        start.elapsed()
    });

    println!(
        "n={n} small_updates_one_by_one={:.2}ms large_update_into_state={:.2}ms failed_import={:.2}ms",
        small.as_secs_f64() * 1e3,
        large.as_secs_f64() * 1e3,
        failed.as_secs_f64() * 1e3,
    );
}
