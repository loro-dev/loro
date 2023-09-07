use loro_common::ID;
use loro_internal::{version::Frontiers, LoroDoc};

fn main() {
    use bench_utils::TextAction;
    use std::time::Instant;

    let actions = bench_utils::get_automerge_actions();
    let mut loro = LoroDoc::default();
    let start = Instant::now();
    // loro.subscribe_deep(Box::new(|_| ()));
    let text = loro.get_text("text");
    for _ in 0..1 {
        let mut txn = loro.txn().unwrap();
        for TextAction { del, ins, pos } in actions.iter() {
            text.delete_utf16(&mut txn, *pos, *del).unwrap();
            text.insert_utf16(&mut txn, *pos, ins).unwrap();
        }
    }
    loro.checkout(&Frontiers::from(ID::new(loro.peer_id(), 100)))
        .unwrap();
    // loro.diagnose();
    println!("{}", start.elapsed().as_millis());
}
