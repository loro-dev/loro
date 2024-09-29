use std::time::Instant;

use dev_utils::{get_mem_usage, ByteSize};
use loro::{LoroDoc, ID};

pub fn main() {
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    let n: u32 = 100_000;
    println!("N = {}", n);

    let start = Instant::now();
    for i in 0..n {
        if i % 2 == 0 {
            list.push(0).unwrap();
        } else {
            list.push(i).unwrap();
        }
    }
    doc.commit();
    println!("Inserted N items to the movable list");
    println!("Time cost {:?}", start.elapsed());

    let start = Instant::now();
    for i in 0..n {
        list.set(i as usize, i + 1).unwrap();
    }
    doc.commit();
    println!("Set N items to the movable list");
    println!("Time cost {:?}", start.elapsed());

    let start = Instant::now();
    for i in (1..n).step_by(2) {
        list.mov(i as usize, i as usize - 1).unwrap();
    }
    doc.commit();
    println!("Moved N items in the movable list");
    println!("Time cost {:?}", start.elapsed());

    let start = Instant::now();
    println!("Memory cost {}", get_mem_usage());
    doc.checkout(&ID::new(doc.peer_id(), (n / 2) as i32).into())
        .unwrap();
    println!("Memory cost after checkout {}", get_mem_usage());
    println!("Time cost {:?}", start.elapsed());
    let start = Instant::now();
    doc.checkout_to_latest();
    println!("Checkout to latest time cost {:?}", start.elapsed());

    let start = Instant::now();
    doc.compact_change_store();
    println!("Memory cost after compact {}", get_mem_usage());
    println!("Time cost {:?}", start.elapsed());
    println!(
        "Kv size {}",
        ByteSize(doc.with_oplog(|log| log.change_store_kv_size()))
    );

    let start = Instant::now();
    let before = get_mem_usage();
    doc.free_history_cache();
    println!("Memory cost after free history cache {}", get_mem_usage());
    let used = before - get_mem_usage();
    println!("History cache size {}", used);
    println!("Time cost {:?}", start.elapsed());

    let start = Instant::now();
    let updates = doc.export(loro::ExportMode::all_updates());
    println!("Export updates time cost {:?}", start.elapsed());
    let start = Instant::now();
    let doc2 = LoroDoc::new();
    doc2.import(&updates).unwrap();
    println!("Import updates time cost {:?}", start.elapsed());
}
