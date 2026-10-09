//! Ad-hoc perf probe for imports that overlap history the doc already has
//! (`OpLog::check_and_trim_known_part_of_changes`, loro-dev/loro#1118).
//!
//! Run on two revisions and compare. Not part of CI.
use loro::{ExportMode, LoroDoc, VersionVector};
use std::time::{Duration, Instant};

/// Two peers editing text, a list and a map, syncing every few commits so the
/// history has many changes with cross-peer deps.
fn make_history(commits: usize) -> (LoroDoc, LoroDoc) {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    for i in 0..commits {
        let doc = if i % 2 == 0 { &a } else { &b };
        let text = doc.get_text("text");
        let len = text.len_unicode();
        text.insert((i * 7) % (len + 1), "hello ").unwrap();
        if len > 20 && i % 3 == 0 {
            text.delete((i * 5) % (len - 10), 3).unwrap();
        }
        doc.get_list("list").push(i as i64).unwrap();
        doc.get_map("map")
            .insert(&format!("k{}", i % 100), i as i64)
            .unwrap();
        doc.commit();
        if i % 4 == 3 {
            a.import(&b.export(ExportMode::updates(&a.oplog_vv())).unwrap())
                .unwrap();
            b.import(&a.export(ExportMode::updates(&b.oplog_vv())).unwrap())
                .unwrap();
        }
    }
    a.import(&b.export(ExportMode::updates(&a.oplog_vv())).unwrap())
        .unwrap();
    (a, b)
}

fn time<T>(rounds: usize, mut f: impl FnMut() -> T) -> Duration {
    let mut best = Duration::MAX;
    for _ in 0..rounds {
        let start = Instant::now();
        std::hint::black_box(f());
        best = best.min(start.elapsed());
    }
    best
}

fn main() {
    let commits: usize = std::env::var("COMMITS")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(20_000);
    let rounds = 7;
    let (source, _) = make_history(commits);
    let base_snapshot = source.export(ExportMode::Snapshot).unwrap();
    let base_vv = source.oplog_vv();
    let fresh_base = || {
        let doc = LoroDoc::new();
        doc.import(&base_snapshot).unwrap();
        doc
    };

    // One more change on top, from a third peer.
    let next = LoroDoc::new();
    next.import(&base_snapshot).unwrap();
    next.set_peer_id(3).unwrap();
    next.get_text("text").insert(0, "new").unwrap();
    next.commit();
    let all_plus_one = next.export(ExportMode::all_updates()).unwrap();
    let snapshot_plus_one = next.export(ExportMode::Snapshot).unwrap();
    let only_new = next.export(ExportMode::updates(&base_vv)).unwrap();
    let all_known = source.export(ExportMode::all_updates()).unwrap();

    // The last 20 commits' worth of history re-sent with the new change.
    let mut resend_from = base_vv.clone();
    for (_, counter) in resend_from.iter_mut() {
        *counter = (*counter - 30).max(0);
    }
    let resend_plus_one = next.export(ExportMode::updates(&resend_from)).unwrap();

    // Many small non-overlapping updates, imported one by one.
    let (inc_src, _) = make_history(2_000);
    let mut blobs = Vec::new();
    let mut vv = VersionVector::default();
    {
        let replay = LoroDoc::new();
        let json = inc_src
            .export_json_updates(&Default::default(), &inc_src.oplog_vv())
            .unwrap();
        for change in json.changes {
            replay
                .import_json_updates(json_schema_with(&json.peers, change))
                .unwrap();
            blobs.push(replay.export(ExportMode::updates(&vv)).unwrap());
            vv = replay.oplog_vv();
        }
    }

    let load = std::fs::read_to_string("/proc/loadavg").ok().or_else(|| {
        std::process::Command::new("sysctl")
            .args(["-n", "vm.loadavg"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    });
    println!(
        "commits={commits} changes={} loadavg={}",
        source.len_changes(),
        load.unwrap_or_default()
    );

    let report =
        |name: &str, d: Duration| println!("{name:<40} {:>10.3} ms", d.as_secs_f64() * 1e3);
    report(
        "incremental: 2k small blobs one by one",
        time(rounds, || {
            let doc = LoroDoc::new();
            for b in &blobs {
                doc.import(b).unwrap();
            }
            doc
        }),
    );
    // Times the import alone, into a doc freshly loaded from the base snapshot.
    let run = |name: &str, bytes: &[u8]| {
        let mut best = Duration::MAX;
        for _ in 0..rounds {
            let doc = fresh_base();
            let start = Instant::now();
            doc.import(bytes).unwrap();
            best = best.min(start.elapsed());
        }
        report(name, best);
    };
    run("only new change (no overlap)", &only_new);
    run("re-sent tail + new change", &resend_plus_one);
    run("all updates, all known (no-op)", &all_known);
    run("all updates + new change", &all_plus_one);
    run("snapshot, all known (no-op)", &base_snapshot);
    run("snapshot + new change", &snapshot_plus_one);
}

fn json_schema_with(peers: &Option<Vec<u64>>, change: loro::JsonChange) -> loro::JsonSchema {
    loro::JsonSchema {
        schema_version: 1,
        start_version: Default::default(),
        peers: peers.clone(),
        changes: vec![change],
    }
}
