use std::time::Instant;

use loro_internal::{LoroCore, LoroValue};
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
        updates.push(actor.encode_from(Default::default()));
    }

    actors[0].decode_batch(&updates).unwrap();

    // drop(profiler);
    println!("{}", start.elapsed().as_millis());
}
