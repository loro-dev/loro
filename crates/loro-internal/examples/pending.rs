use bench_utils::TextAction;
use loro_internal::{cursor::PosType, encoding::ExportMode, LoroDoc, VersionVector};

pub fn main() {
    let loro = LoroDoc::default();
    let mut latest_vv = VersionVector::default();
    let mut updates = vec![];
    let actions = bench_utils::get_automerge_actions();
    let action_length = actions.len();
    let text = loro.get_text("text");
    for chunks in actions.chunks(action_length / 10) {
        for TextAction { pos, ins, del } in chunks {
            let mut txn = loro.txn().unwrap();
            text.delete_with_txn(&mut txn, *pos, *del, PosType::Unicode)
                .unwrap();
            text.insert_with_txn(&mut txn, *pos, ins, PosType::Unicode)
                .unwrap();
            let update = loro.export(ExportMode::updates(&latest_vv)).unwrap();
            updates.push(update);
            latest_vv = loro.oplog_vv();
        }
    }

    println!("done encoding");
    updates.reverse();
    let start = std::time::Instant::now();
    let store2 = LoroDoc::default();
    store2.detach();
    for update in updates.iter() {
        store2.import(update).unwrap();
    }
    println!("Elapsed {}", start.elapsed().as_millis());
}
