//! Import-path scaling probe for loro-crdt 1.16.4 vs 1.16.3 (`ad5b2a6d`).
//!
//! Each process prints one `RESULT` line. `rounds` (default 3) are timed after one
//! warmup; the line is median / min / mean. `FAT` is the inserted string length
//! (default 1). History uses merge interval -1 so each commit is its own change.
//!
//! ```text
//! CARGO_BUILD_JOBS=4 cargo run -p examples --release --example import_scaling_stress -- <scenario> <n> [rounds]
//! ```
//!
//! Scenarios: `text_peers`, `map_peers`, `text_stream`, `overlap_mem`, `overlap_snap`,
//! `overlap_partial`, `snap_plus`, `snap_mem`, `batch`, `batch_rev`, `ooo`, `detached`,
//! `mlist_batch`, `mlist_stream`, `check`.
use loro::{ExportMode, LoroDoc, ID};
use std::time::{Duration, Instant};

fn main() {
    let mut args = std::env::args().skip(1);
    let scenario = args.next().unwrap_or_else(|| "help".into());
    if scenario == "help" || scenario == "-h" {
        eprintln!(
            "usage: import_scaling_stress <scenario> <n> [rounds]\n\
             env: FAT=<bytes per insert, default 1>  IMPORTS=<text_peers import count, default 200>"
        );
        std::process::exit(2);
    }
    let n: usize = args
        .next()
        .unwrap_or_else(|| "1000".into())
        .parse()
        .expect("n");
    let rounds: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(3);
    let fat: usize = std::env::var("FAT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
        .max(1);
    println!(
        "META scenario={scenario} n={n} rounds={rounds} fat={fat} load={}",
        loadavg()
    );
    match scenario.as_str() {
        "text_peers" => peers(n, rounds, true),
        "map_peers" => peers(n, rounds, false),
        "text_stream" => text_stream(n, rounds, fat),
        "overlap_mem" => overlap(n, rounds, fat, OverlapMode::Mem),
        "overlap_snap" => overlap(n, rounds, fat, OverlapMode::Snap),
        "overlap_partial" => overlap(n, rounds, fat, OverlapMode::Partial),
        "snap_plus" => snap_plus(n, rounds, fat),
        "snap_mem" => snap_mem(n, fat),
        "batch" => batch(n, rounds, false),
        "batch_rev" => batch(n, rounds, true),
        "ooo" => ooo(n, rounds),
        "detached" => detached(n, rounds),
        "mlist_batch" => mlist(n, rounds, false),
        "mlist_stream" => mlist(n, rounds, true),
        "mlist_hole" => mlist_hole(n, true),
        "check" => check(),
        other => {
            eprintln!("unknown scenario {other}");
            std::process::exit(2);
        }
    }
}

fn loadavg() -> String {
    std::fs::read_to_string("/proc/loadavg")
        .ok()
        .or_else(|| {
            std::process::Command::new("sysctl")
                .args(["-n", "vm.loadavg"])
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })
        .unwrap_or_default()
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(",")
}

fn rss_kb() -> u64 {
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

#[repr(C)]
struct MallocStats {
    blocks_in_use: u32,
    size_in_use: usize,
    max_size_in_use: usize,
    size_allocated: usize,
}

extern "C" {
    fn malloc_default_zone() -> *mut std::ffi::c_void;
    fn malloc_zone_statistics(zone: *mut std::ffi::c_void, stats: *mut MallocStats);
}

fn heap_kb() -> u64 {
    unsafe {
        let mut stats = MallocStats {
            blocks_in_use: 0,
            size_in_use: 0,
            max_size_in_use: 0,
            size_allocated: 0,
        };
        malloc_zone_statistics(malloc_default_zone(), &mut stats);
        (stats.size_in_use / 1024) as u64
    }
}

struct Samples {
    median: Duration,
    min: Duration,
    mean: Duration,
}

fn samples_of(mut times: Vec<Duration>) -> Samples {
    times.sort();
    let total: Duration = times.iter().copied().sum();
    Samples {
        median: times[times.len() / 2],
        min: times[0],
        mean: total / times.len() as u32,
    }
}

fn report(scenario: &str, n: usize, samples: &Samples, extra: &str) {
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    println!(
        "RESULT scenario={scenario} n={n} median_ms={:.3} min_ms={:.3} mean_ms={:.3} rss_kb={} heap_kb={} {extra}",
        ms(samples.median),
        ms(samples.min),
        ms(samples.mean),
        rss_kb(),
        heap_kb(),
    );
}

fn chunk(fat: usize) -> String {
    if fat == 1 {
        "a".to_string()
    } else {
        "x".repeat(fat)
    }
}

fn append_text(doc: &LoroDoc, s: &str) {
    let text = doc.get_text("t");
    let len = text.len_unicode();
    text.insert(len, s).unwrap();
    doc.commit();
}

/// `n` commits from `peer`. Merge interval -1 keeps each commit its own change
/// (0 still merges commits that share a timestamp second).
fn text_history(n: usize, peer: u64, fat: usize) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer).unwrap();
    doc.set_change_merge_interval(-1);
    let piece = chunk(fat);
    for _ in 0..n {
        append_text(&doc, &piece);
    }
    doc
}

fn peers(peer_count: usize, rounds: usize, text_imports: bool) {
    let imports: usize = std::env::var("IMPORTS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let setup = Instant::now();
    let base = LoroDoc::new();
    for p in 1..=peer_count {
        let d = LoroDoc::new();
        d.set_peer_id(p as u64).unwrap();
        d.get_map("m").insert("k", p as i64).unwrap();
        d.commit();
        base.import(&d.export(ExportMode::all_updates()).unwrap())
            .unwrap();
    }
    let frontiers = base.oplog_frontiers().len();
    let src = LoroDoc::new();
    src.set_peer_id(1).unwrap();
    src.set_change_merge_interval(-1);
    src.get_map("m").insert("k", 1i64).unwrap();
    src.commit();
    let mut vv = src.oplog_vv();
    let mut blobs = Vec::with_capacity(imports);
    for i in 0..imports {
        if text_imports {
            src.get_text("t").insert(i, "a").unwrap();
        } else {
            src.get_map("m").insert(&format!("n{i}"), i as i64).unwrap();
        }
        src.commit();
        blobs.push(src.export(ExportMode::updates(&vv)).unwrap());
        vv = src.oplog_vv();
    }
    let expect_text = {
        let d = base.fork();
        for b in &blobs {
            d.import(b).unwrap();
        }
        if text_imports {
            d.get_text("t").to_string()
        } else {
            String::new()
        }
    };
    println!(
        "SETUP peers={peer_count} frontiers={frontiers} imports={imports} setup_ms={:.1}",
        setup.elapsed().as_secs_f64() * 1e3
    );
    let mut times = Vec::with_capacity(rounds + 1);
    for round in 0..=rounds {
        let doc = base.fork();
        let start = Instant::now();
        for b in &blobs {
            doc.import(b).unwrap();
        }
        let elapsed = start.elapsed();
        if text_imports {
            assert_eq!(doc.get_text("t").to_string(), expect_text);
        }
        std::hint::black_box(doc.state_frontiers());
        if round > 0 {
            times.push(elapsed);
        }
    }
    report(
        if text_imports {
            "text_peers"
        } else {
            "map_peers"
        },
        peer_count,
        &samples_of(times),
        &format!("imports={imports} frontiers={frontiers}"),
    );
}

fn text_stream(n: usize, rounds: usize, fat: usize) {
    let piece = chunk(fat);
    // Remote imports merge when the timestamp delta is <= 0. Spreading the
    // commit timestamps keeps each update as its own op instead of one growing
    // insert, which is what makes a per-import clone of the last op quadratic.
    let spread = std::env::var("SPREAD").ok().as_deref() == Some("1");
    let src = LoroDoc::new();
    src.set_peer_id(2).unwrap();
    src.set_change_merge_interval(-1);
    let seedsync = std::env::var("SEEDSYNC").ok().as_deref() == Some("1");
    let mut seed_blob = Vec::new();
    if seedsync {
        let sd = LoroDoc::new();
        sd.set_peer_id(1).unwrap();
        sd.get_text("t").insert(0, "seed").unwrap();
        sd.commit();
        seed_blob = sd.export(ExportMode::all_updates()).unwrap();
        src.import(&seed_blob).unwrap();
    }
    let mut vv = src.oplog_vv();
    let mut blobs = Vec::with_capacity(n);
    for i in 0..n {
        let text = src.get_text("t");
        let len = text.len_unicode();
        text.insert(len, &piece).unwrap();
        if spread {
            src.set_next_commit_timestamp((i as i64) * 10);
        }
        src.commit();
        blobs.push(src.export(ExportMode::updates(&vv)).unwrap());
        vv = src.oplog_vv();
    }
    let expected = format!("seed{}", src.get_text("t").to_string());
    let expected = if seedsync {
        src.get_text("t").to_string()
    } else {
        expected
    };
    let mut times = Vec::with_capacity(rounds + 1);
    let mut recv_changes = 0;
    for round in 0..=rounds {
        let doc = LoroDoc::new();
        doc.set_peer_id(1).unwrap();
        let noseed = std::env::var("NOSEED").ok().as_deref() == Some("1");
        if seedsync {
            doc.import(&seed_blob).unwrap();
        } else if !noseed {
            doc.get_text("t").insert(0, "seed").unwrap();
            doc.commit();
        }
        let start = Instant::now();
        for b in &blobs {
            doc.import(b).unwrap();
        }
        let elapsed = start.elapsed();
        if noseed || seedsync {
            assert_eq!(doc.get_text("t").to_string(), src.get_text("t").to_string());
        } else {
            assert_eq!(doc.get_text("t").to_string(), expected);
        }
        recv_changes = doc.len_changes();
        if round > 0 {
            times.push(elapsed);
        }
    }
    report(
        "text_stream",
        n,
        &samples_of(times),
        &format!(
            "fat={fat} spread={spread} src_changes={} recv_changes={recv_changes}",
            src.len_changes()
        ),
    );
}

#[derive(Clone, Copy)]
enum OverlapMode {
    Mem,
    Snap,
    Partial,
}

fn overlap(n: usize, rounds: usize, fat: usize, mode: OverlapMode) {
    let src = text_history(n, 1, fat);
    let base_vv = src.oplog_vv();
    let base_end = *base_vv.get(&1).expect("peer 1");
    let changes = src.len_changes();
    let base_updates = src.export(ExportMode::all_updates()).unwrap();
    let base_snap = src.export(ExportMode::Snapshot).unwrap();
    append_text(&src, "Z");
    let all_plus = src.export(ExportMode::all_updates()).unwrap();
    let expected = src.get_text("t").to_string();
    let mut mid = base_vv;
    mid.set_end(ID::new(1, base_end / 2));
    let partial = src.export(ExportMode::updates(&mid)).unwrap();
    let (name, blob_len) = match mode {
        OverlapMode::Mem => ("overlap_mem", all_plus.len()),
        OverlapMode::Snap => ("overlap_snap", all_plus.len()),
        OverlapMode::Partial => ("overlap_partial", partial.len()),
    };
    let mut times = Vec::with_capacity(rounds + 1);
    for round in 0..=rounds {
        let doc = LoroDoc::new();
        match mode {
            OverlapMode::Mem => {
                doc.import(&base_updates).unwrap();
            }
            OverlapMode::Snap | OverlapMode::Partial => {
                doc.import(&base_snap).unwrap();
            }
        }
        // Leave the loaded doc cold: do not touch len_changes / get_change here.
        let blob = match mode {
            OverlapMode::Partial => &partial,
            _ => &all_plus,
        };
        let start = Instant::now();
        doc.import(blob).unwrap();
        let elapsed = start.elapsed();
        assert_eq!(doc.get_text("t").to_string(), expected);
        if round > 0 {
            times.push(elapsed);
        }
    }
    report(
        name,
        n,
        &samples_of(times),
        &format!(
            "fat={fat} changes={changes} blob={blob_len} snap={}",
            base_snap.len()
        ),
    );
}

fn snap_plus(n: usize, rounds: usize, fat: usize) {
    let src = text_history(n, 1, fat);
    let changes = src.len_changes();
    let base_snap = src.export(ExportMode::Snapshot).unwrap();
    append_text(&src, "Z");
    let snap_plus = src.export(ExportMode::Snapshot).unwrap();
    let expected = src.get_text("t").to_string();
    let mut times = Vec::with_capacity(rounds + 1);
    let mut last_heap = 0;
    for round in 0..=rounds {
        let doc = LoroDoc::new();
        doc.import(&base_snap).unwrap();
        let before = heap_kb();
        let start = Instant::now();
        doc.import(&snap_plus).unwrap();
        let elapsed = start.elapsed();
        last_heap = heap_kb().saturating_sub(before);
        assert_eq!(doc.get_text("t").to_string(), expected);
        if round > 0 {
            times.push(elapsed);
        }
        drop(doc);
    }
    report(
        "snap_plus",
        n,
        &samples_of(times),
        &format!(
            "fat={fat} changes={changes} snap={} snap_plus={} heap_delta_kb={last_heap}",
            base_snap.len(),
            snap_plus.len()
        ),
    );
}

/// Import a growing snapshot (full history + one new op) into the same doc.
/// Heap after each import is the retained-memory signal.
fn snap_mem(n: usize, fat: usize) {
    let repeats: usize = std::env::var("REPEATS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(6);
    let src = text_history(n, 1, fat);
    let base_snap = src.export(ExportMode::Snapshot).unwrap();
    let mut snaps = Vec::with_capacity(repeats);
    let mut only_new = Vec::with_capacity(repeats);
    for _ in 0..repeats {
        let vv = src.oplog_vv();
        append_text(&src, "Z");
        snaps.push(src.export(ExportMode::Snapshot).unwrap());
        only_new.push(src.export(ExportMode::updates(&vv)).unwrap());
    }
    let doc = LoroDoc::new();
    doc.import(&base_snap).unwrap();
    println!(
        "MEM kind=snap_base heap_kb={} rss_kb={} snap={}",
        heap_kb(),
        rss_kb(),
        base_snap.len()
    );
    for (i, snap) in snaps.iter().enumerate() {
        let before = heap_kb();
        let start = Instant::now();
        doc.import(snap).unwrap();
        let ms = start.elapsed().as_secs_f64() * 1e3;
        println!(
            "MEM kind=snap i={i} import_ms={ms:.3} heap_kb={} delta_kb={} rss_kb={} bytes={}",
            heap_kb(),
            heap_kb() as i64 - before as i64,
            rss_kb(),
            snap.len()
        );
    }
    assert_eq!(doc.get_text("t").to_string(), src.get_text("t").to_string());

    let control = LoroDoc::new();
    control.import(&base_snap).unwrap();
    let before = heap_kb();
    for (i, upd) in only_new.iter().enumerate() {
        let start = Instant::now();
        control.import(upd).unwrap();
        println!(
            "MEM kind=update i={i} import_ms={:.3} heap_kb={} rss_kb={} bytes={}",
            start.elapsed().as_secs_f64() * 1e3,
            heap_kb(),
            rss_kb(),
            upd.len()
        );
    }
    println!(
        "MEM kind=update_total delta_kb={}",
        heap_kb() as i64 - before as i64
    );
    assert_eq!(
        control.get_text("t").to_string(),
        src.get_text("t").to_string()
    );
}

fn one_peer_blobs(n: usize) -> Vec<Vec<u8>> {
    let src = LoroDoc::new();
    src.set_peer_id(2).unwrap();
    src.set_change_merge_interval(-1);
    let mut vv = src.oplog_vv();
    let mut blobs = Vec::with_capacity(n);
    for _ in 0..n {
        append_text(&src, "a");
        blobs.push(src.export(ExportMode::updates(&vv)).unwrap());
        vv = src.oplog_vv();
    }
    blobs
}

fn seeded() -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.get_text("t").insert(0, "seed").unwrap();
    doc.commit();
    doc
}

fn batch(n: usize, rounds: usize, reverse: bool) {
    let mut blobs = one_peer_blobs(n);
    if reverse {
        blobs.reverse();
    }
    let expected = {
        let doc = seeded();
        doc.import_batch(&blobs).unwrap();
        doc.get_text("t").to_string()
    };
    let mut times = Vec::with_capacity(rounds + 1);
    for round in 0..=rounds {
        let doc = seeded();
        let start = Instant::now();
        doc.import_batch(&blobs).unwrap();
        let elapsed = start.elapsed();
        assert_eq!(doc.get_text("t").to_string(), expected);
        if round > 0 {
            times.push(elapsed);
        }
    }
    report(
        if reverse { "batch_rev" } else { "batch" },
        n,
        &samples_of(times),
        &format!("blobs={n}"),
    );
}

fn ooo(n: usize, rounds: usize) {
    let mut blobs = one_peer_blobs(n);
    blobs.reverse();
    let expected = {
        let doc = seeded();
        for b in &blobs {
            doc.import(b).unwrap();
        }
        doc.get_text("t").to_string()
    };
    let mut times = Vec::with_capacity(rounds + 1);
    for round in 0..=rounds {
        let doc = seeded();
        let start = Instant::now();
        for b in &blobs {
            doc.import(b).unwrap();
        }
        let elapsed = start.elapsed();
        assert_eq!(doc.get_text("t").to_string(), expected);
        if round > 0 {
            times.push(elapsed);
        }
    }
    report("ooo", n, &samples_of(times), &format!("blobs={n}"));
}

fn detached(n: usize, rounds: usize) {
    let blobs = one_peer_blobs(n);
    let expected = {
        let doc = seeded();
        doc.detach();
        for b in &blobs {
            doc.import(b).unwrap();
        }
        doc.attach();
        doc.get_text("t").to_string()
    };
    let mut import_times = Vec::with_capacity(rounds + 1);
    let mut attach_times = Vec::with_capacity(rounds + 1);
    for round in 0..=rounds {
        let doc = seeded();
        doc.detach();
        let start = Instant::now();
        for b in &blobs {
            doc.import(b).unwrap();
        }
        let import_elapsed = start.elapsed();
        let start = Instant::now();
        doc.attach();
        let attach_elapsed = start.elapsed();
        assert_eq!(doc.get_text("t").to_string(), expected);
        assert!(!doc.is_detached());
        if round > 0 {
            import_times.push(import_elapsed);
            attach_times.push(attach_elapsed);
        }
    }
    let imports = samples_of(import_times);
    let attaches = samples_of(attach_times);
    report(
        "detached",
        n,
        &imports,
        &format!(
            "attach_median_ms={:.3} attach_min_ms={:.3}",
            attaches.median.as_secs_f64() * 1e3,
            attaches.min.as_secs_f64() * 1e3
        ),
    );
}

fn mlist_elements(n: usize) -> (LoroDoc, Vec<u8>) {
    let src = LoroDoc::new();
    src.set_peer_id(1).unwrap();
    src.set_change_merge_interval(-1);
    let list = src.get_movable_list("m");
    for i in 0..n {
        list.insert(i, i as i64).unwrap();
        src.commit();
    }
    let snap = src.export(ExportMode::Snapshot).unwrap();
    (src, snap)
}

fn mlist(n: usize, rounds: usize, stream: bool) {
    let (src, snap) = mlist_elements(n);
    let changes = src.len_changes();
    let base_vv = src.oplog_vv();
    let editor = LoroDoc::new();
    editor.import(&snap).unwrap();
    editor.set_peer_id(2).unwrap();
    editor.set_change_merge_interval(-1);
    let list = editor.get_movable_list("m");
    let mut per_op = Vec::with_capacity(n);
    let mut vv = editor.oplog_vv();
    for i in 0..n {
        list.set(i, (i as i64) + 1).unwrap();
        editor.commit();
        per_op.push(editor.export(ExportMode::updates(&vv)).unwrap());
        vv = editor.oplog_vv();
    }
    let batch_blob = editor.export(ExportMode::updates(&base_vv)).unwrap();
    let expected = editor.get_movable_list("m").get_deep_value();
    let mut times = Vec::with_capacity(rounds + 1);
    for round in 0..=rounds {
        let doc = LoroDoc::new();
        doc.import(&snap).unwrap();
        let start = Instant::now();
        if stream {
            for b in &per_op {
                doc.import(b).unwrap();
            }
        } else {
            doc.import(&batch_blob).unwrap();
        }
        let elapsed = start.elapsed();
        assert_eq!(doc.get_movable_list("m").get_deep_value(), expected);
        if round > 0 {
            times.push(elapsed);
        }
    }
    report(
        if stream {
            "mlist_stream"
        } else {
            "mlist_batch"
        },
        n,
        &samples_of(times),
        &format!(
            "changes={changes} snap={} blob={}",
            snap.len(),
            if stream {
                per_op.iter().map(Vec::len).sum::<usize>()
            } else {
                batch_blob.len()
            }
        ),
    );
}

fn check() {
    let mut failed = 0;
    let mut run = |name: &str, f: fn()| {
        let start = Instant::now();
        match std::panic::catch_unwind(f) {
            Ok(()) => println!("PASS {name} {:.1}ms", start.elapsed().as_secs_f64() * 1e3),
            Err(payload) => {
                failed += 1;
                let msg = payload
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| payload.downcast_ref::<&str>().copied())
                    .unwrap_or("panic");
                println!("FAIL {name}: {msg}");
            }
        }
    };
    run("overlap_snap_equals", overlap_snap_equals);
    run("overlap_partial_equals", overlap_partial_equals);
    run("merged_change_prefix", merged_change_prefix);
    run("deletes_roundtrip_overlap", deletes_roundtrip_overlap);
    run("random_text_overlap", random_text_overlap);
    run("snap_repeat_equals", snap_repeat_equals);
    run("mlist_sets_after_snapshot", mlist_sets_after_snapshot);
    run("mlist_moves_after_snapshot", mlist_moves_after_snapshot);
    run("mlist_after_get_change", mlist_after_get_change);
    run("mlist_set_before_insert", mlist_set_before_insert);
    run("batch_rev_equals_forward", batch_rev_equals_forward);
    run("ooo_equals_forward", ooo_equals_forward);
    run("detached_equals_attached", detached_equals_attached);
    run("two_peers_snapshot_overlap", two_peers_snapshot_overlap);
    run("conflict_same_peer", conflict_same_peer);
    if failed > 0 {
        eprintln!("CHECK_FAILS {failed}");
        std::process::exit(1);
    }
    println!("CHECK_OK");
}

fn overlap_snap_equals() {
    let src = text_history(80, 1, 8);
    let snap = src.export(ExportMode::Snapshot).unwrap();
    append_text(&src, "Q");
    let doc = LoroDoc::new();
    doc.import(&snap).unwrap();
    doc.import(&src.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(doc.get_text("t").to_string(), src.get_text("t").to_string());
}

fn overlap_partial_equals() {
    let src = text_history(80, 1, 4);
    let vv = src.oplog_vv();
    let end = *vv.get(&1).unwrap();
    let snap = src.export(ExportMode::Snapshot).unwrap();
    append_text(&src, "Q");
    let mut mid = vv;
    mid.set_end(ID::new(1, end / 2));
    let doc = LoroDoc::new();
    doc.import(&snap).unwrap();
    doc.import(&src.export(ExportMode::updates(&mid)).unwrap())
        .unwrap();
    assert_eq!(doc.get_text("t").to_string(), src.get_text("t").to_string());
}

fn merged_change_prefix() {
    let src = LoroDoc::new();
    src.set_peer_id(1).unwrap();
    src.set_change_merge_interval(10_000);
    for _ in 0..40 {
        append_text(&src, "m");
    }
    assert_eq!(src.len_changes(), 1, "expected one merged change");
    let end = *src.oplog_vv().get(&1).unwrap();
    let receiver = LoroDoc::new();
    receiver.set_peer_id(1).unwrap();
    receiver.set_change_merge_interval(10_000);
    for _ in 0..(end as usize / 2) {
        append_text(&receiver, "m");
    }
    append_text(&src, "Z");
    receiver
        .import(&src.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(
        receiver.get_text("t").to_string(),
        src.get_text("t").to_string()
    );
}

fn deletes_roundtrip_overlap() {
    let src = LoroDoc::new();
    src.set_peer_id(1).unwrap();
    src.set_change_merge_interval(-1);
    for _ in 0..30 {
        append_text(&src, "abcd");
    }
    let text = src.get_text("t");
    for i in 0..10 {
        let len = text.len_unicode();
        text.delete(len / 2, (i % 3) + 1).unwrap();
        src.commit();
    }
    let snap = src.export(ExportMode::Snapshot).unwrap();
    append_text(&src, "Z");
    let doc = LoroDoc::new();
    doc.import(&snap).unwrap();
    doc.import(&src.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(doc.get_text("t").to_string(), src.get_text("t").to_string());
}

fn random_text_overlap() {
    for seed in [1u64, 2, 3, 7, 11, 19, 23, 42] {
        let mut state = seed;
        let mut next = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            state
        };
        let src = LoroDoc::new();
        src.set_peer_id(1).unwrap();
        src.set_change_merge_interval(-1);
        for _ in 0..60 {
            let text = src.get_text("t");
            let len = text.len_unicode();
            if len > 4 && next() % 5 == 0 {
                let at = (next() as usize) % len;
                let n = (1 + (next() as usize) % 3).min(len - at);
                text.delete(at, n).unwrap();
            } else {
                let at = if len == 0 {
                    0
                } else {
                    (next() as usize) % (len + 1)
                };
                let s = if next() % 2 == 0 { "xy" } else { "z" };
                text.insert(at, s).unwrap();
            }
            src.commit();
        }
        let snap = src.export(ExportMode::Snapshot).unwrap();
        append_text(&src, "Q");
        let doc = LoroDoc::new();
        doc.import(&snap).unwrap();
        doc.import(&src.export(ExportMode::Snapshot).unwrap())
            .unwrap();
        assert_eq!(
            doc.get_text("t").to_string(),
            src.get_text("t").to_string(),
            "seed {seed}"
        );
    }
}

fn snap_repeat_equals() {
    let src = text_history(40, 1, 8);
    let doc = LoroDoc::new();
    doc.import(&src.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    for _ in 0..5 {
        append_text(&src, "Z");
        doc.import(&src.export(ExportMode::Snapshot).unwrap())
            .unwrap();
        assert_eq!(doc.get_text("t").to_string(), src.get_text("t").to_string());
    }
}

fn mlist_sets_after_snapshot() {
    let (src, snap) = mlist_elements(120);
    let vv = src.oplog_vv();
    let editor = LoroDoc::new();
    editor.import(&snap).unwrap();
    editor.set_peer_id(2).unwrap();
    let list = editor.get_movable_list("m");
    for i in 0..120 {
        list.set(i, (i as i64) * 10).unwrap();
    }
    editor.commit();
    let doc = LoroDoc::new();
    doc.import(&snap).unwrap();
    doc.import(&editor.export(ExportMode::updates(&vv)).unwrap())
        .unwrap();
    assert_eq!(
        doc.get_movable_list("m").get_deep_value(),
        editor.get_movable_list("m").get_deep_value()
    );
}

fn mlist_moves_after_snapshot() {
    let (src, snap) = mlist_elements(60);
    let vv = src.oplog_vv();
    let editor = LoroDoc::new();
    editor.import(&snap).unwrap();
    editor.set_peer_id(2).unwrap();
    let list = editor.get_movable_list("m");
    for i in 0..20 {
        list.mov(i, 59 - i).unwrap();
    }
    editor.commit();
    let doc = LoroDoc::new();
    doc.import(&snap).unwrap();
    doc.import(&editor.export(ExportMode::updates(&vv)).unwrap())
        .unwrap();
    assert_eq!(
        doc.get_movable_list("m").get_deep_value(),
        editor.get_movable_list("m").get_deep_value()
    );
}

fn mlist_after_get_change() {
    // Enough inserts that the history is more than one 4KB block.
    let (src, snap) = mlist_elements(800);
    assert!(src.len_changes() > 1);
    let vv = src.oplog_vv();
    let editor = LoroDoc::new();
    editor.import(&snap).unwrap();
    editor.set_peer_id(2).unwrap();
    let list = editor.get_movable_list("m");
    for i in (0..800).step_by(7) {
        list.set(i, -1i64).unwrap();
    }
    editor.commit();
    let blob = editor.export(ExportMode::updates(&vv)).unwrap();
    let doc = LoroDoc::new();
    doc.import(&snap).unwrap();
    // Parse an early block, then import sets of later elements.
    assert!(doc.get_change(ID::new(1, 0)).is_some());
    doc.import(&blob).unwrap();
    assert_eq!(
        doc.get_movable_list("m").get_deep_value(),
        editor.get_movable_list("m").get_deep_value()
    );
}

/// Snapshot-load `n` elements, optionally parse an early block, then import a
/// set of every element. A lamport lookup that stops on the cached early block
/// rejects the later sets.
fn mlist_hole(n: usize, prime: bool) {
    let (src, snap) = mlist_elements(n);
    let vv = src.oplog_vv();
    let editor = LoroDoc::new();
    editor.import(&snap).unwrap();
    editor.set_peer_id(2).unwrap();
    let list = editor.get_movable_list("m");
    for i in 0..n {
        list.set(i, -1i64).unwrap();
    }
    editor.commit();
    let blob = editor.export(ExportMode::updates(&vv)).unwrap();
    let doc = LoroDoc::new();
    doc.import(&snap).unwrap();
    if prime {
        assert!(doc.get_change(ID::new(1, 0)).is_some());
    }
    doc.import(&blob).unwrap();
    assert_eq!(
        doc.get_movable_list("m").get_deep_value(),
        editor.get_movable_list("m").get_deep_value()
    );
    println!(
        "PASS mlist_hole n={n} prime={prime} changes={}",
        src.len_changes()
    );
}

fn mlist_set_before_insert() {
    let src = LoroDoc::new();
    src.set_peer_id(1).unwrap();
    src.get_movable_list("m").insert(0, "a").unwrap();
    src.commit();
    let insert_blob = src.export(ExportMode::all_updates()).unwrap();
    let vv = src.oplog_vv();
    src.get_movable_list("m").set(0, "b").unwrap();
    src.commit();
    let set_blob = src.export(ExportMode::updates(&vv)).unwrap();
    let doc = LoroDoc::new();
    doc.import(&set_blob).unwrap();
    doc.import(&insert_blob).unwrap();
    assert_eq!(
        doc.get_movable_list("m").get_deep_value(),
        src.get_movable_list("m").get_deep_value()
    );
}

fn batch_rev_equals_forward() {
    let blobs = one_peer_blobs(40);
    let mut rev = blobs.clone();
    rev.reverse();
    let a = seeded();
    a.import_batch(&blobs).unwrap();
    let b = seeded();
    b.import_batch(&rev).unwrap();
    assert_eq!(a.get_text("t").to_string(), b.get_text("t").to_string());
}

fn ooo_equals_forward() {
    let blobs = one_peer_blobs(40);
    let mut rev = blobs.clone();
    rev.reverse();
    let a = seeded();
    for b in &blobs {
        a.import(b).unwrap();
    }
    let b = seeded();
    for blob in &rev {
        b.import(blob).unwrap();
    }
    assert_eq!(a.get_text("t").to_string(), b.get_text("t").to_string());
}

fn detached_equals_attached() {
    let blobs = one_peer_blobs(30);
    let attached = seeded();
    for b in &blobs {
        attached.import(b).unwrap();
    }
    let detached = seeded();
    detached.detach();
    for b in &blobs {
        detached.import(b).unwrap();
    }
    detached.attach();
    assert_eq!(
        attached.get_text("t").to_string(),
        detached.get_text("t").to_string()
    );
}

fn two_peers_snapshot_overlap() {
    let a = text_history(40, 1, 4);
    let b = text_history(40, 2, 4);
    let both = LoroDoc::new();
    both.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    both.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    append_text(&both, "Z");
    let receiver = LoroDoc::new();
    receiver
        .import(&a.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    receiver
        .import(&both.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_eq!(
        receiver.get_text("t").to_string(),
        both.get_text("t").to_string()
    );
}

fn conflict_same_peer() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    a.get_text("t").insert(0, "aaa").unwrap();
    a.commit();
    let b = LoroDoc::new();
    b.set_peer_id(1).unwrap();
    b.get_text("t").insert(0, "bbb").unwrap();
    b.commit();
    b.get_text("t").insert(3, "X").unwrap();
    b.commit();
    let doc = LoroDoc::new();
    doc.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let result = doc.import(&b.export(ExportMode::all_updates()).unwrap());
    println!("INFO conflict_same_peer {result:?}");
}
