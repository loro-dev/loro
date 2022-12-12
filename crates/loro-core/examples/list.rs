use std::time::Instant;
// #[global_allocator]
// static ALLOC: dhat::Alloc = dhat::Alloc;

use loro_core::{LoroCore, VersionVector};

fn main() {
    // let p = dhat::Profiler::builder().trim_backtraces(None).build();
    let start = Instant::now();
    let mut actor = LoroCore::default();
    let mut output = Vec::new();
    let mut list = actor.get_list("list");
    let mut last_vv = actor.vv().encode();
    for i in 0..10000 {
        list.insert(&actor, i, i.to_string()).unwrap();
        output.push(actor.export_updates(&VersionVector::decode(&last_vv).unwrap()));
        last_vv = actor.vv().encode();
    }
    println!("{} ms", start.elapsed().as_millis());
    // drop(p)
}
