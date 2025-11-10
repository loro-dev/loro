use std::time::Instant;
// #[global_allocator]
// static ALLOC: dhat::Alloc = dhat::Alloc;

use loro_internal::{encoding::ExportMode, LoroDoc};

fn main() {
    // let p = dhat::Profiler::builder().trim_backtraces(None).build();
    let start = Instant::now();
    let actor = LoroDoc::default();
    let mut output = Vec::new();
    let list = actor.get_list("list");
    let mut last_vv = actor.oplog_vv();
    for i in 0..10000 {
        let mut txn = actor.txn().unwrap();
        list.insert_with_txn(&mut txn, i, i.to_string().into())
            .unwrap();
        output.push(actor.export(ExportMode::updates(&last_vv.clone())).unwrap());
        last_vv = actor.oplog_vv();
    }
    println!("{} ms", start.elapsed().as_millis());
    // drop(p)
}
