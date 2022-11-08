// use tikv_jemallocator::Jemalloc;
// #[global_allocator]
// static GLOBAL: Jemalloc = Jemalloc;

#[cfg(feature = "mem-prof")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

const RAW_DATA: &[u8; 901823] = include_bytes!("../benches/automerge-paper.json.gz");

use std::{io::Read, time::Instant};

use flate2::read::GzDecoder;
use loro_core::LoroCore;
use serde_json::Value;

pub fn main() {
    // let alloc_stats = stats::allocated::mib().unwrap();
    let mut d = GzDecoder::new(&RAW_DATA[..]);
    let mut s = String::new();
    d.read_to_string(&mut s).unwrap();
    let json: Value = serde_json::from_str(&s).unwrap();
    drop(s);
    let txns = json.as_object().unwrap().get("txns");
    let start = Instant::now();
    #[cfg(feature = "mem-prof")]
    let profiler = dhat::Profiler::builder().trim_backtraces(None).build();
    let mut loro = LoroCore::default();
    let text = loro.get_or_create_root_text("text");
    let mut text_lock = text.lock().unwrap();
    let text = text_lock.as_text_mut().unwrap();
    for _i in 0..1 {
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

            if start.elapsed().as_secs() > 10 {
                break;
            }
        }
    }
    drop(json);
    drop(d);
    drop(text_lock);
    #[cfg(feature = "fuzzing")]
    loro.debug_inspect();
    #[cfg(feature = "mem-prof")]
    drop(profiler);
    // e.advance().unwrap();
    // let new_new_heap = alloc_stats.read().unwrap();
    println!("Apply Automerge Dataset 1X");
    // println!("Mem: {} MB", new_new_heap as f64 / 1024. / 1024.);
    println!("Used: {} ms", start.elapsed().as_millis());
}
