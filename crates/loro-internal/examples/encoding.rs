use bench_utils::TextAction;
use loro_internal::LoroDoc;

fn main() {
    let actions = bench_utils::get_automerge_actions();
    let loro = LoroDoc::default();
    let text = loro.get_text("text");
    let mut txn = loro.txn().unwrap();

    for TextAction { pos, ins, del } in actions.iter() {
        text.delete(&mut txn, *pos, *del).unwrap();
        text.insert(&mut txn, *pos, ins).unwrap();
    }

    txn.commit().unwrap();
    for _ in 0..10 {
        loro.export_from(&Default::default());
    }
    let data = loro.export_from(&Default::default());
    for _ in 0..100 {
        let mut b = LoroDoc::default();
        b.detach();
        b.import(&data).unwrap();
    }
}
