#[cfg(not(target_env = "msvc"))]
use jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

use jemalloc_ctl::{epoch, stats};
const RAW_DATA: &[u8; 901823] = include_bytes!("../benches/automerge-paper.json.gz");

use std::{io::Read, time::Instant};

use flate2::read::GzDecoder;
use loro_core::LoroCore;
use serde_json::Value;

pub fn main() {
    let alloc_stats = stats::allocated::mib().unwrap();
    let mut d = GzDecoder::new(&RAW_DATA[..]);
    let mut s = String::new();
    d.read_to_string(&mut s).unwrap();
    let json: Value = serde_json::from_str(&s).unwrap();
    drop(s);
    let txns = json.as_object().unwrap().get("txns");
    let e = epoch::mib().unwrap();
    let start = Instant::now();
    let mut loro = LoroCore::default();
    let mut text = loro.get_or_create_root_text("text").unwrap();
    for i in 0..10 {
        for txn in txns.unwrap().as_array().unwrap() {
            let patches = txn
                .as_object()
                .unwrap()
                .get("patches")
                .unwrap()
                .as_array()
                .unwrap();
            for patch in patches {
                let pos = patch[0].as_u64().unwrap() as usize;
                let del_here = patch[1].as_u64().unwrap() as usize;
                let ins_content = patch[2].as_str().unwrap();
                text.delete(pos, del_here);
                text.insert(pos, ins_content);
            }
        }
    }
    drop(json);
    drop(d);
    e.advance().unwrap();
    let new_new_heap = alloc_stats.read().unwrap();
    println!("Apply Automerge Dataset 10X");
    println!("Mem: {} MB", new_new_heap as f64 / 1024. / 1024.);
    println!("Used: {} ms", start.elapsed().as_millis());
}
