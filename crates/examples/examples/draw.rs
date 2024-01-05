use std::time::Instant;

use bench_utils::{json, SyncKind};
use examples::{draw::DrawActor, run_async_workflow, run_realtime_collab_workflow};
use loro::{LoroDoc, ToJson};
use tabled::{settings::Style, Table, Tabled};

#[derive(Tabled)]
struct BenchResult {
    task: &'static str,
    action_size: usize,
    peer_num: usize,
    ops_num: usize,
    changes_num: usize,
    snapshot_size: usize,
    updates_size: usize,
    apply_duration: f64,
    encode_snapshot_duration: f64,
    encode_udpate_duration: f64,
    decode_snapshot_duration: f64,
    decode_update_duration: f64,
    doc_json_size: usize,
}

pub fn main() {
    let seed = 123123;
    let ans = vec![
        // run_async(1, 100, seed),
        // run_async(1, 1000, seed),
        // run_async(1, 5000, seed),
        // run_async(1, 10000, seed),
        // run_async(5, 100, seed),
        // run_async(5, 1000, seed),
        // run_async(5, 10000, seed),
        // run_async(10, 1000, seed),
        // run_async(10, 10000, seed),
        // run_async(10, 100000, seed),
        // run_async(10, 100000, 1000),
        // run_realtime_collab(5, 100, seed),
        // run_realtime_collab(5, 1000, seed),
        // run_realtime_collab(5, 10000, seed),
        // run_realtime_collab(10, 1000, seed),
        run_realtime_collab(10, 10000, seed),
        // run_realtime_collab(10, 100000, seed),
        // run_realtime_collab(10, 100000, 1000),
    ];
    let mut table = Table::new(ans);
    let style = Style::markdown();
    table.with(style);
    println!("{}", table);
}

fn run_async(peer_num: usize, action_num: usize, seed: u64) -> BenchResult {
    eprintln!(
        "run_async(peer_num: {}, action_num: {})",
        peer_num, action_num
    );
    let (mut actors, start) =
        run_async_workflow::<DrawActor>(peer_num, action_num, 200, seed, |action| {
            if let bench_utils::Action::Sync { kind, .. } = action {
                *kind = SyncKind::Fit;
            }
        });
    actors.sync_all();
    let apply_duration = start.elapsed().as_secs_f64() * 1000.;

    let start = Instant::now();
    let snapshot = actors.docs[0].doc.export_snapshot();
    let encode_snapshot_duration = start.elapsed().as_secs_f64() * 1000.;
    let snapshot_size = snapshot.len();

    let start = Instant::now();
    let updates = actors.docs[0].doc.export_from(&Default::default());
    let encode_udpate_duration = start.elapsed().as_secs_f64() * 1000.;
    let updates_size = updates.len();

    let start = Instant::now();
    let doc = LoroDoc::new();
    doc.import(&snapshot).unwrap();
    let decode_snapshot_duration = start.elapsed().as_secs_f64() * 1000.;

    let doc = LoroDoc::new();
    let start = Instant::now();
    doc.import(&updates).unwrap();
    let decode_update_duration = start.elapsed().as_secs_f64() * 1000.;
    let value = doc.get_deep_value();
    let json = value.to_json().len();
    BenchResult {
        task: "async draw",
        action_size: action_num,
        peer_num,
        snapshot_size,
        ops_num: actors.docs[0].doc.len_ops(),
        changes_num: actors.docs[0].doc.len_changes(),
        updates_size,
        apply_duration,
        encode_snapshot_duration,
        encode_udpate_duration,
        decode_snapshot_duration,
        decode_update_duration,
        doc_json_size: json,
    }
}

fn run_realtime_collab(peer_num: usize, action_num: usize, seed: u64) -> BenchResult {
    eprintln!(
        "run_realtime_collab(peer_num: {}, action_num: {})",
        peer_num, action_num
    );
    let (mut actors, start) =
        run_realtime_collab_workflow::<DrawActor>(peer_num, action_num, seed, |action| {
            if let bench_utils::Action::Sync { kind, .. } = action {
                *kind = SyncKind::Fit;
            }
        });
    actors.sync_all();
    let apply_duration = start.elapsed().as_secs_f64() * 1000.;

    let start = Instant::now();
    let snapshot = actors.docs[0].doc.export_snapshot();
    let encode_snapshot_duration = start.elapsed().as_secs_f64() * 1000.;
    let snapshot_size = snapshot.len();

    let start = Instant::now();
    let updates = actors.docs[0].doc.export_from(&Default::default());
    let encode_udpate_duration = start.elapsed().as_secs_f64() * 1000.;
    let updates_size = updates.len();

    let start = Instant::now();
    let doc = LoroDoc::new();
    doc.import(&snapshot).unwrap();
    let decode_snapshot_duration = start.elapsed().as_secs_f64() * 1000.;

    let doc = LoroDoc::new();
    let start = Instant::now();
    doc.import(&updates).unwrap();
    let decode_update_duration = start.elapsed().as_secs_f64() * 1000.;
    let json_len = doc.get_deep_value().to_json().len();
    doc.log_estimate_size();

    BenchResult {
        task: "realtime draw",
        action_size: action_num,
        peer_num,
        ops_num: actors.docs[0].doc.len_ops(),
        changes_num: actors.docs[0].doc.len_changes(),
        snapshot_size,
        updates_size,
        apply_duration,
        encode_snapshot_duration,
        encode_udpate_duration,
        decode_snapshot_duration,
        decode_update_duration,
        doc_json_size: json_len,
    }
}
