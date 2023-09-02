use std::time::Instant;

use loro_internal::{LoroDoc, LoroValue};
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    let mut actors: Vec<_> = (0..1000).map(|_| LoroDoc::default()).collect();
    let mut updates: Vec<Vec<u8>> = Vec::new();
    for (i, actor) in actors.iter_mut().enumerate() {
        let list = actor.get_list("list");
        let value: LoroValue = i.to_string().into();
        let mut txn = actor.txn().unwrap();
        list.insert(&mut txn, 0, value).unwrap();
        txn.commit().unwrap();
        updates.push(actor.export_from(&Default::default()));
    }

    drop(actors);

    let profiler = dhat::Profiler::builder().trim_backtraces(None).build();
    let start = Instant::now();
    let mut actor = LoroDoc::default();
    actor.import_batch(&updates).unwrap();
    println!("{} bytes", updates.iter().map(|x| x.len()).sum::<usize>());
    // dbg!(actor.get_state_deep_value());
    println!("{} ms", start.elapsed().as_millis());
    drop(profiler);
}
