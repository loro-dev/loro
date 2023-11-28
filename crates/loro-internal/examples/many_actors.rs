use std::time::Instant;

use loro_internal::{LoroDoc, LoroValue};
// #[global_allocator]
// static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    with_100k_actors_then_action();
    // import_with_many_actors();
}

#[allow(unused)]
fn import_with_many_actors() {
    let store = LoroDoc::default();
    for i in 0..10000 {
        store.set_peer_id(i);
        let list = store.get_list("list");
        let value: LoroValue = i.to_string().into();
        let mut txn = store.txn().unwrap();
        list.insert_with_txn(&mut txn, 0, value).unwrap();
        txn.commit().unwrap();
    }

    {
        let start = Instant::now();
        let bytes = store.export_snapshot();
        LoroDoc::default().import(&bytes).unwrap();
        println!("{} ms", start.elapsed().as_millis());
    }

    // let profiler = dhat::Profiler::builder().trim_backtraces(None).build();
    // let start = Instant::now();
    // let mut actor = LoroDoc::default();
    // actor.import_batch(&updates).unwrap();
    // println!("{} bytes", updates.iter().map(|x| x.len()).sum::<usize>());
    // // dbg!(actor.get_state_deep_value());
    // println!("{} ms", start.elapsed().as_millis());
    // drop(profiler);
}

#[allow(unused)]
fn with_100k_actors_then_action() {
    let store = LoroDoc::default();
    for i in 0..100_000 {
        store.set_peer_id(i);
        let list = store.get_list("list");
        let value: LoroValue = i.to_string().into();
        let mut txn = store.txn().unwrap();
        list.insert_with_txn(&mut txn, 0, value).unwrap();
        txn.commit().unwrap();
    }

    for i in 0..200_000 {
        let list = store.get_list("list");
        let value: LoroValue = i.to_string().into();
        let mut txn = store.txn().unwrap();
        list.insert_with_txn(&mut txn, 0, value).unwrap();
        txn.commit().unwrap();
    }
}
