use std::time::Instant;

use loro_core::{log_store::EncodeConfig, LoroCore, LoroValue};
// #[global_allocator]
// static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    let start = Instant::now();
    // let profiler = dhat::Profiler::builder().trim_backtraces(None).build();
    let mut actors: Vec<_> = (0..1540).map(|_| LoroCore::default()).collect();
    let mut updates: Vec<Vec<u8>> = Vec::new();
    for (i, actor) in actors.iter_mut().enumerate() {
        let mut list = actor.get_list("list");
        let value: LoroValue = i.to_string().into();
        list.insert(actor, 0, value).unwrap();
        updates.push(actor.encode(EncodeConfig::from_vv(None)).unwrap());
    }

    actors[0].import_updates_batch(&updates).unwrap();

    // drop(profiler);
    println!("{}", start.elapsed().as_millis());
}
