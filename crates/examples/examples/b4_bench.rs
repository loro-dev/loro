//! B4 (automerge-paper) performance harness.
//!
//! Usage:
//!   cargo run --release -p examples --example b4_bench            # phase report
//!   cargo run --release -p examples --example b4_bench edit       # tight edit loop (for profiler)
//!   cargo run --release -p examples --example b4_bench import     # tight import loop (for profiler)
//!   cargo run --release -p examples --example b4_bench import100  # tight import loop for B4x100
use std::time::{Duration, Instant};

use bench_utils::{get_automerge_actions, TextAction};
use dev_utils::{get_mem_usage, ByteSize};
use loro::{ExportMode, LoroDoc};

fn apply(actions: &[TextAction], n: usize) -> LoroDoc {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    for _ in 0..n {
        for TextAction { del, ins, pos } in actions.iter() {
            text.delete(*pos, *del).unwrap();
            text.insert(*pos, ins).unwrap();
        }
    }
    doc.commit();
    doc
}

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

fn time<T>(runs: usize, mut f: impl FnMut() -> T) -> (Duration, T) {
    let mut last = None;
    let mut times = Vec::new();
    for _ in 0..runs {
        let start = Instant::now();
        let r = f();
        times.push(start.elapsed());
        last = Some(r);
    }
    (median(times), last.unwrap())
}

fn report() {
    let actions = get_automerge_actions();
    let total_ops: usize = actions.len();
    println!("B4 actions: {total_ops} (each = 1 delete + 1 insert)\n");

    // ---- Local editing ----
    let mem0 = get_mem_usage();
    let (t_apply, doc) = time(5, || apply(&actions, 1));
    let mem_after_apply = get_mem_usage() - mem0;
    println!("== Local editing (one big txn, no subscriber) ==");
    println!(
        "  apply 1x:            {:>10.2?}   ({:.2} M op/s, {:.0} ns/op)",
        t_apply,
        (2 * total_ops) as f64 / t_apply.as_secs_f64() / 1e6,
        t_apply.as_nanos() as f64 / (2 * total_ops) as f64
    );
    println!("  doc mem after apply: {}", mem_after_apply);

    // ---- Snapshot export ----
    let (t_export, snapshot) = time(5, || doc.export(ExportMode::Snapshot).unwrap());
    println!("\n== Snapshot export ==");
    println!("  export (has cache):  {:>10.2?}", t_export);
    println!("  snapshot size:       {}", ByteSize(snapshot.len()));

    let (t_export_nc, _) = time(5, || {
        let d = apply(&actions, 1);
        d.export(ExportMode::Snapshot).unwrap()
    });
    println!(
        "  export(+apply,nocache):{:>8.2?}  (includes a fresh apply)",
        t_export_nc
    );

    // ---- Snapshot import ----
    let mem_before = get_mem_usage();
    let (t_import, imported) = time(5, || {
        let d = LoroDoc::new();
        d.import(&snapshot).unwrap();
        d
    });
    let mem_imported = get_mem_usage() - mem_before;
    println!("\n== Snapshot import (B4) ==");
    println!("  import:              {:>10.2?}", t_import);
    println!("  mem after import:    {}", mem_imported);

    let (t_import_val, _) = time(5, || {
        let d = LoroDoc::new();
        d.import(&snapshot).unwrap();
        let v = d.get_deep_value();
        std::hint::black_box(v);
    });
    println!(
        "  import + toJSON:     {:>10.2?}  (forces full state materialization)",
        t_import_val
    );
    std::hint::black_box(&imported);

    // ---- B4 x100 ----
    let (t_apply100, doc100) = time(1, || apply(&actions, 100));
    let snap100 = doc100.export(ExportMode::Snapshot).unwrap();
    println!("\n== B4 x100 ==");
    println!("  apply 100x:          {:>10.2?}", t_apply100);
    println!("  snapshot size:       {}", ByteSize(snap100.len()));
    let (t_import100, _) = time(5, || {
        let d = LoroDoc::new();
        d.import(&snap100).unwrap();
        d
    });
    println!("  import:              {:>10.2?}", t_import100);
    let (t_import100_val, _) = time(5, || {
        let d = LoroDoc::new();
        d.import(&snap100).unwrap();
        std::hint::black_box(d.get_deep_value());
    });
    println!("  import + toJSON:     {:>10.2?}", t_import100_val);

    // ---- updates encode/decode (history path) ----
    let updates = doc.export(ExportMode::all_updates()).unwrap();
    println!("\n== Updates (history) ==");
    println!("  updates size:        {}", ByteSize(updates.len()));
    let (t_dec_updates, _) = time(5, || {
        let d = LoroDoc::new();
        d.import(&updates).unwrap();
        d
    });
    println!("  import updates:      {:>10.2?}", t_dec_updates);
}

/// Tight loop over `f` for `secs` seconds. Use with an external sampling
/// profiler, e.g.:
///   cargo instruments -t time --release -p examples --example b4_bench -- edit 20
fn loop_for(secs: u64, _label: &str, mut f: impl FnMut()) {
    let start = Instant::now();
    let mut iters = 0u64;
    while start.elapsed() < Duration::from_secs(secs) {
        f();
        iters += 1;
    }
    eprintln!("ran {iters} iters in {:?}", start.elapsed());
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let secs: u64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(12);
    match mode.as_str() {
        "edit" => {
            let actions = get_automerge_actions();
            loop_for(secs, "edit", || {
                std::hint::black_box(apply(&actions, 1));
            });
        }
        "import" => {
            let actions = get_automerge_actions();
            let snapshot = apply(&actions, 1).export(ExportMode::Snapshot).unwrap();
            loop_for(secs, "import", || {
                let d = LoroDoc::new();
                d.import(&snapshot).unwrap();
                std::hint::black_box(d);
            });
        }
        "import100" => {
            let actions = get_automerge_actions();
            let snapshot = apply(&actions, 100).export(ExportMode::Snapshot).unwrap();
            loop_for(secs, "import100", || {
                let d = LoroDoc::new();
                d.import(&snapshot).unwrap();
                std::hint::black_box(d);
            });
        }
        "import_val" => {
            let actions = get_automerge_actions();
            let snapshot = apply(&actions, 1).export(ExportMode::Snapshot).unwrap();
            loop_for(secs, "import_val", || {
                let d = LoroDoc::new();
                d.import(&snapshot).unwrap();
                std::hint::black_box(d.get_deep_value());
            });
        }
        _ => report(),
    }
}
