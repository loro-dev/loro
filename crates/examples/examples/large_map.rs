use std::time::Instant;

use dev_utils::get_mem_usage;
use loro::LoroDoc;

fn main() {
    let mut init = Instant::now();
    {
        let start = Instant::now();
        let doc = LoroDoc::new();
        let map = doc.get_map("map");
        for i in 0..1000 {
            for j in 0..1000 {
                let key = format!("0000{}:0000{}", i, j);
                map.insert(&key, i + j).unwrap();
            }
        }

        println!("LargeMap Init Time {:?}", start.elapsed());
        let start = Instant::now();
        let bytes = doc.export(loro::ExportMode::Snapshot).unwrap();
        println!("LargeMap Export Time {:?}", start.elapsed());
        let start = Instant::now();
        let new_doc = LoroDoc::new();
        new_doc.import(&bytes).unwrap();
        println!("LargeMap Import Time {:?}", start.elapsed());
        println!("Mem {:?}", get_mem_usage());
        init = Instant::now();
    }
    println!("Drop Time {:?}", init.elapsed());
    println!("Mem {:?}", get_mem_usage())
}
