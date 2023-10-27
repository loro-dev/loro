use std::sync::Arc;

use bench_utils::TextAction;
use criterion::black_box;
use loro_internal::LoroDoc;

fn main() {
    let actions = bench_utils::get_automerge_actions();
    for _ in 0..10 {
        let loro = LoroDoc::default();
        let text = loro.get_text("text");
        loro.subscribe_deep(Arc::new(move |event| {
            black_box(event);
        }));
        for TextAction { pos, ins, del } in actions.iter() {
            let mut txn = loro.txn().unwrap();
            text.delete(&mut txn, *pos, *del).unwrap();
            text.insert(&mut txn, *pos, ins).unwrap();
        }
    }
}
