use loro_internal::LoroDoc;

fn main() {
    use bench_utils::TextAction;
    use std::time::Instant;

    let actions = bench_utils::get_automerge_actions();
    let loro = LoroDoc::default();
    let start = Instant::now();
    // loro.subscribe_deep(Box::new(|_| ()));
    let text = loro.get_text("text");
    let n = 100;
    for _ in 0..n {
        let mut txn = loro.txn().unwrap();
        for TextAction { del, ins, pos } in actions.iter() {
            text.delete_with_txn(&mut txn, *pos, *del).unwrap();
            text.insert_with_txn(&mut txn, *pos, ins).unwrap();
        }
    }
    println!("Apply time {}", start.elapsed().as_millis());
    loro.diagnose_size();
    drop(actions);
    let start = Instant::now();
    for _ in 0..1 {
        loro.export_snapshot();
    }
    println!("Snapshot encoding time {}", start.elapsed().as_millis());
}
