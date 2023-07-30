use std::time::Instant;
// #[global_allocator]
// static ALLOC: dhat::Alloc = dhat::Alloc;

use loro_internal::LoroDoc;

fn main() {
    // let p = dhat::Profiler::builder().trim_backtraces(None).build();
    let start = Instant::now();
    let mut actor = LoroDoc::default();
    let mut output = Vec::new();
    let mut list = actor.get_list("list");
    let mut last_vv = actor.vv_cloned();
    for i in 0..10000 {
        let mut txn = actor.txn().unwrap();
        list.insert(&mut txn, i, i.to_string().into()).unwrap();
        output.push(actor.export_from(&last_vv.clone()));
        last_vv = actor.vv_cloned();
    }
    println!("{} ms", start.elapsed().as_millis());
    // drop(p)
}
