use dev_utils::{get_mem_usage, ByteSize};
use loro::LoroDoc;
use std::time::Instant;

pub fn main() {
    let node_num = 1_000_000;
    let op_per_node = 100;

    let bytes = {
        let start = Instant::now();
        let doc = LoroDoc::new();
        let tree = doc.get_tree("tree");
        let mut nodes = vec![];
        for i in 0..node_num {
            if i % 1000 == 0 {
                doc.set_peer_id(i).unwrap();
            }

            nodes.push(tree.create(None).unwrap());
        }

        println!("Create 1M Nodes Duration {:?}", start.elapsed());

        for i in 0..op_per_node {
            println!(
                "Percentage: {:.2}% {:?}",
                i as f64 / op_per_node as f64 * 100.0,
                get_mem_usage()
            );
            for (k, node) in nodes.iter().enumerate() {
                if k % 1000 == 0 && k != 0 {
                    doc.set_peer_id(k as u64 - 1).unwrap();
                }

                let map = tree.get_meta(*node).unwrap();
                map.insert("key", "value".to_string()).unwrap();
                map.insert("counter", i).unwrap();
            }
            doc.compact_change_store();
        }

        println!("Total Ops.len={}", doc.len_ops());
        println!("100M ops duration {:?}", start.elapsed());
        println!("Mem {:?}", get_mem_usage());

        let start = Instant::now();
        let snapshot = doc.export(loro::ExportMode::Snapshot).unwrap();
        println!("Export snapshot duration {:?}", start.elapsed());
        println!("Mem {:?}", get_mem_usage());

        let start = Instant::now();
        let new_doc = LoroDoc::new();
        new_doc.import(&snapshot).unwrap();
        println!("Import snapshot duration {:?}", start.elapsed());

        let start = Instant::now();
        let _s = new_doc.export(loro::ExportMode::Snapshot);
        println!("New doc export snapshot time {:?}", start.elapsed());
        println!("Mem {:?}", get_mem_usage());
        snapshot
    };

    let shallow_snapshot = {
        println!("Snapshot size {:?}", ByteSize(bytes.len()));
        let doc = LoroDoc::new();
        doc.import(&bytes).unwrap();
        println!("Mem usage after import snapshot {:?}", get_mem_usage());
        let start = Instant::now();
        let _v = doc.get_deep_value();
        println!("GetValue duration {:?}", start.elapsed());
        println!("Mem usage after getting value {:?}", get_mem_usage());
        let start = Instant::now();
        let _trimmed_bytes = doc.export(loro::ExportMode::Snapshot).unwrap();
        println!("ReExport Snapshot Duration {:?}", start.elapsed());
        let start = Instant::now();
        let shallow_bytes = doc
            .export(loro::ExportMode::shallow_snapshot(&doc.oplog_frontiers()))
            .unwrap();
        println!("Export ShallowSnapshot Duration {:?}", start.elapsed());
        println!("ShallowSnapshot size {:?}", ByteSize(shallow_bytes.len()));
        shallow_bytes
    };

    {
        let start = Instant::now();
        let doc = LoroDoc::new();
        doc.import(&shallow_snapshot).unwrap();
        println!("Import gc snapshot time: {:?}", start.elapsed());
        println!("Mem usage {:?}", get_mem_usage());
        let start = Instant::now();
        let _v = doc.get_deep_value();
        println!("GetValue duration {:?}", start.elapsed());
        println!("Mem usage after getting value {:?}", get_mem_usage());
    }
}

// Benchmark Result
//
// commit a369f39263366bfa53b6a083f739bb3caceffa0d
// 2024-10-05 00:03:45
// Macbook Pro M1, 2020
//
// ```
// Create 1M Nodes Duration 2.008214333s
// Percentage: 0.00% 612.72 MB
// Percentage: 1.00% 1.30 GB
// Percentage: 2.00% 1.37 GB
// Percentage: 3.00% 1.42 GB
// Percentage: 4.00% 1.47 GB
// Percentage: 5.00% 1.53 GB
// Percentage: 6.00% 1.58 GB
// Percentage: 7.00% 1.64 GB
// Percentage: 8.00% 1.69 GB
// Percentage: 9.00% 1.74 GB
// Percentage: 10.00% 1.80 GB
// Percentage: 11.00% 1.85 GB
// Percentage: 12.00% 1.91 GB
// Percentage: 13.00% 1.96 GB
// Percentage: 14.00% 2.01 GB
// Percentage: 15.00% 2.07 GB
// Percentage: 16.00% 2.12 GB
// Percentage: 17.00% 2.18 GB
// Percentage: 18.00% 2.23 GB
// Percentage: 19.00% 2.28 GB
// Percentage: 20.00% 2.34 GB
// Percentage: 21.00% 2.39 GB
// Percentage: 22.00% 2.45 GB
// Percentage: 23.00% 2.50 GB
// Percentage: 24.00% 2.55 GB
// Percentage: 25.00% 2.61 GB
// Percentage: 26.00% 2.66 GB
// Percentage: 27.00% 2.72 GB
// Percentage: 28.00% 2.77 GB
// Percentage: 29.00% 2.82 GB
// Percentage: 30.00% 2.88 GB
// Percentage: 31.00% 2.93 GB
// Percentage: 32.00% 2.99 GB
// Percentage: 33.00% 3.04 GB
// Percentage: 34.00% 3.09 GB
// Percentage: 35.00% 3.15 GB
// Percentage: 36.00% 3.20 GB
// Percentage: 37.00% 3.25 GB
// Percentage: 38.00% 3.31 GB
// Percentage: 39.00% 3.36 GB
// Percentage: 40.00% 3.42 GB
// Percentage: 41.00% 3.47 GB
// Percentage: 42.00% 3.53 GB
// Percentage: 43.00% 3.58 GB
// Percentage: 44.00% 3.63 GB
// Percentage: 45.00% 3.69 GB
// Percentage: 46.00% 3.74 GB
// Percentage: 47.00% 3.80 GB
// Percentage: 48.00% 3.85 GB
// Percentage: 49.00% 3.90 GB
// Percentage: 50.00% 3.96 GB
// Percentage: 51.00% 4.01 GB
// Percentage: 52.00% 4.06 GB
// Percentage: 53.00% 4.12 GB
// Percentage: 54.00% 4.17 GB
// Percentage: 55.00% 4.23 GB
// Percentage: 56.00% 4.28 GB
// Percentage: 57.00% 4.33 GB
// Percentage: 58.00% 4.39 GB
// Percentage: 59.00% 4.44 GB
// Percentage: 60.00% 4.50 GB
// Percentage: 61.00% 4.55 GB
// Percentage: 62.00% 4.60 GB
// Percentage: 63.00% 4.66 GB
// Percentage: 64.00% 4.71 GB
// Percentage: 65.00% 4.77 GB
// Percentage: 66.00% 4.84 GB
// Percentage: 67.00% 4.90 GB
// Percentage: 68.00% 4.96 GB
// Percentage: 69.00% 5.02 GB
// Percentage: 70.00% 5.08 GB
// Percentage: 71.00% 5.14 GB
// Percentage: 72.00% 5.20 GB
// Percentage: 73.00% 5.27 GB
// Percentage: 74.00% 5.33 GB
// Percentage: 75.00% 5.39 GB
// Percentage: 76.00% 5.45 GB
// Percentage: 77.00% 5.51 GB
// Percentage: 78.00% 5.57 GB
// Percentage: 79.00% 5.64 GB
// Percentage: 80.00% 5.70 GB
// Percentage: 81.00% 5.76 GB
// Percentage: 82.00% 5.82 GB
// Percentage: 83.00% 5.88 GB
// Percentage: 84.00% 5.94 GB
// Percentage: 85.00% 6.01 GB
// Percentage: 86.00% 6.07 GB
// Percentage: 87.00% 6.13 GB
// Percentage: 88.00% 6.19 GB
// Percentage: 89.00% 6.25 GB
// Percentage: 90.00% 6.31 GB
// Percentage: 91.00% 6.38 GB
// Percentage: 92.00% 6.44 GB
// Percentage: 93.00% 6.50 GB
// Percentage: 94.00% 6.56 GB
// Percentage: 95.00% 6.62 GB
// Percentage: 96.00% 6.68 GB
// Percentage: 97.00% 6.75 GB
// Percentage: 98.00% 6.81 GB
// Percentage: 99.00% 6.87 GB
// Total Ops.len=102000000
// 100M ops duration 166.533197667s
// Mem 6.93 GB
// Export snapshot duration 6.022444208s
// Mem 7.44 GB
// Import snapshot duration 771.09ms
// New doc export snapshot time 5.959888834s
// Mem 10.59 GB
// Snapshot size 425.78 MB
// Mem usage after import snapshot 1.38 GB
// GetValue duration 2.802668584s
// Mem usage after getting value 3.37 GB
// ReExport Snapshot Duration 6.449782042s
// Export TrimmedSnapshot Duration 4.207467s
// TrimmedSnapshot size 16.86 MB
// Import gc snapshot time: 2.143892458s
// Mem usage 1.05 GB
// GetValue duration 2.86928975s
// Mem usage after getting value 3.04 GB
// ```
