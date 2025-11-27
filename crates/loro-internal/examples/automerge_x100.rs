use loro_internal::{cursor::PosType, loro::ExportMode, LoroDoc, VersionVector};
use std::hint::black_box;

fn main() {
    use bench_utils::TextAction;
    use std::time::Instant;

    let actions = bench_utils::get_automerge_actions();
    let loro = LoroDoc::default();
    loro.start_auto_commit();
    let start = Instant::now();
    // loro.subscribe_deep(Box::new(|_| ()));
    let text = loro.get_text("text");
    let n = 100;
    let mut v = VersionVector::new();
    for _ in 0..n {
        for TextAction { del, ins, pos } in actions.iter() {
            text.delete(*pos, *del, PosType::Unicode).unwrap();
            text.insert(*pos, ins, PosType::Unicode).unwrap();
        }
        loro.commit_then_renew();
        black_box(loro.export(ExportMode::updates(&v))).unwrap();
        v = loro.oplog_vv();
    }
    println!("Apply time {:?}", start.elapsed());
    loro.diagnose_size();
    drop(actions);
    let start = Instant::now();
    let snapshot = loro.export(ExportMode::Snapshot).unwrap();
    println!("Snapshot encoding time {}", start.elapsed().as_millis());
    let compressed = zstd::encode_all(&mut snapshot.as_slice(), 0).unwrap();
    println!(
        "Snapshot encoding time including compression {}",
        start.elapsed().as_millis()
    );
    println!("Snapshot size {}", snapshot.len());
    println!("Snapshot size after compression {}", compressed.len());
    let start = Instant::now();
    let _doc = LoroDoc::from_snapshot(&snapshot);
    println!("Snapshot importing time {:?}", start.elapsed());
}
