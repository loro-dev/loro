use std::time::Instant;
// #[global_allocator]
// static ALLOC: dhat::Alloc = dhat::Alloc;

use loro_core::LoroCore;

fn main() {
    // let p = dhat::Profiler::builder().trim_backtraces(None).build();
    let start = Instant::now();
    let mut actor = LoroCore::default();
    let mut output = Vec::new();
    let mut list = actor.get_list("list");
    let mut last_vv = actor.vv_cloned();
    for i in 0..10000 {
        list.insert(&actor, i, i.to_string()).unwrap();
        output.push(actor.encode_from(last_vv.clone()));
        last_vv = actor.vv_cloned();
    }
    println!("{} ms", start.elapsed().as_millis());
    // drop(p)
}
