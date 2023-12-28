use std::time::Instant;

use benches::draw::{run_async_draw_workflow, run_realtime_collab_draw_workflow};
use loro::LoroDoc;
use tabled::{Table, Tabled};

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
}

pub fn main() {
    let seed = 123123;
    let ans = vec![
        run_async(1, 100, seed),
        run_async(1, 1000, seed),
        run_async(1, 10000, seed),
        run_async(5, 100, seed),
        run_async(5, 1000, seed),
        run_async(5, 10000, seed),
        run_async(10, 1000, seed),
        run_async(10, 10000, seed),
        run_async(10, 100000, seed),
        run_async(10, 100000, 1000),
        run_realtime_collab(5, 100, seed),
        run_realtime_collab(5, 1000, seed),
        run_realtime_collab(5, 10000, seed),
        run_realtime_collab(10, 1000, seed),
        run_realtime_collab(10, 10000, seed),
        run_realtime_collab(10, 100000, seed),
        run_realtime_collab(10, 100000, 1000),
    ];
    println!("{}", Table::new(ans));
}

fn run_async(peer_num: usize, action_num: usize, seed: u64) -> BenchResult {
    eprintln!(
        "run_async(peer_num: {}, action_num: {})",
        peer_num, action_num
    );
    let (mut actors, start) = run_async_draw_workflow(peer_num, action_num, 200, seed);
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

    let start = Instant::now();
    doc.import(&updates).unwrap();
    let decode_update_duration = start.elapsed().as_secs_f64() * 1000.;

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
    }
}

fn run_realtime_collab(peer_num: usize, action_num: usize, seed: u64) -> BenchResult {
    eprintln!(
        "run_realtime_collab(peer_num: {}, action_num: {})",
        peer_num, action_num
    );
    let (mut actors, start) = run_realtime_collab_draw_workflow(peer_num, action_num, seed);
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

    let start = Instant::now();
    doc.import(&updates).unwrap();
    let decode_update_duration = start.elapsed().as_secs_f64() * 1000.;

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
    }
}
