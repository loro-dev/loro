use std::time::Instant;

use loro_internal::{LoroDoc, LoroValue};
// #[global_allocator]
// static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    let start = Instant::now();
    // let profiler = dhat::Profiler::builder().trim_backtraces(None).build();
    let mut actors: Vec<_> = (0..1540).map(|_| LoroDoc::default()).collect();
    let mut updates: Vec<Vec<u8>> = Vec::new();
    for (i, actor) in actors.iter_mut().enumerate() {
        let list = actor.get_list("list");
        let value: LoroValue = i.to_string().into();
        let mut txn = actor.txn().unwrap();
        list.insert(&mut txn, 0, value).unwrap();
        updates.push(actor.export_from(&Default::default()));
    }

    // drop(profiler);
    println!("{}", start.elapsed().as_millis());

    todo!();
    // actors[0].decode_batch(&updates).unwrap();
}
