use bench_utils::TextAction;

use loro_internal::{cursor::PosType, LoroDoc};

fn main() {
    let actions = bench_utils::get_automerge_actions();
    let start = std::time::Instant::now();
    for _ in 0..10 {
        let loro = LoroDoc::default();
        let text = loro.get_text("text");
        // loro.subscribe_deep(Arc::new(move |event| {
        //     black_box(event);
        // }));
        for TextAction { pos, ins, del } in actions.iter() {
            let mut txn = loro.txn().unwrap();
            text.delete_with_txn(&mut txn, *pos, *del, PosType::Unicode)
                .unwrap();
            text.insert_with_txn(&mut txn, *pos, ins, PosType::Unicode)
                .unwrap();
        }

        text.diagnose();
    }

    println!("time: {:?}", start.elapsed());
}
