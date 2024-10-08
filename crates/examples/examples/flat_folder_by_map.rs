use dev_utils::{get_mem_usage, ByteSize};
use loro::{LoroDoc, LoroMap};
use std::time::Instant;

pub fn main() {
    let node_num = 1_000_000;
    let op_per_node = 100;

    let bytes = {
        let start = Instant::now();
        let doc = LoroDoc::new();
        let files = doc.get_map("files");
        let mut nodes = vec![];
        for i in 0..node_num {
            if i % 1000 == 0 {
                doc.set_peer_id(i).unwrap();
            }

            nodes.push(
                files
                    .insert_container(&i.to_string(), LoroMap::new())
                    .unwrap(),
            );
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

                node.insert("key", "value".to_string()).unwrap();
                node.insert("counter", i).unwrap();
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
        println!("Export TrimmedSnapshot Duration {:?}", start.elapsed());
        println!("TrimmedSnapshot size {:?}", ByteSize(shallow_bytes.len()));
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

// commit 950b6b493183ea57149e17dfb86f338720fcbe0a
// 2024-10-05 12:37:10
// Macbook Pro M1, 2020
//
// Create 1M Nodes Duration 1.305751958s
// Percentage: 0.00% 308.73 MB
// Percentage: 1.00% 1.11 GB
// Percentage: 2.00% 1.17 GB
// Percentage: 3.00% 1.23 GB
// Percentage: 4.00% 1.28 GB
// Percentage: 5.00% 1.33 GB
// Percentage: 6.00% 1.39 GB
// Percentage: 7.00% 1.44 GB
// Percentage: 8.00% 1.50 GB
// Percentage: 9.00% 1.55 GB
// Percentage: 10.00% 1.60 GB
// Percentage: 11.00% 1.66 GB
// Percentage: 12.00% 1.71 GB
// Percentage: 13.00% 1.77 GB
// Percentage: 14.00% 1.82 GB
// Percentage: 15.00% 1.87 GB
// Percentage: 16.00% 1.93 GB
// Percentage: 17.00% 1.98 GB
// Percentage: 18.00% 2.04 GB
// Percentage: 19.00% 2.09 GB
// Percentage: 20.00% 2.14 GB
// Percentage: 21.00% 2.20 GB
// Percentage: 22.00% 2.25 GB
// Percentage: 23.00% 2.31 GB
// Percentage: 24.00% 2.36 GB
// Percentage: 25.00% 2.41 GB
// Percentage: 26.00% 2.47 GB
// Percentage: 27.00% 2.52 GB
// Percentage: 28.00% 2.58 GB
// Percentage: 29.00% 2.63 GB
// Percentage: 30.00% 2.68 GB
// Percentage: 31.00% 2.74 GB
// Percentage: 32.00% 2.79 GB
// Percentage: 33.00% 2.85 GB
// Percentage: 34.00% 2.90 GB
// Percentage: 35.00% 2.95 GB
// Percentage: 36.00% 3.01 GB
// Percentage: 37.00% 3.06 GB
// Percentage: 38.00% 3.12 GB
// Percentage: 39.00% 3.17 GB
// Percentage: 40.00% 3.22 GB
// Percentage: 41.00% 3.28 GB
// Percentage: 42.00% 3.33 GB
// Percentage: 43.00% 3.39 GB
// Percentage: 44.00% 3.44 GB
// Percentage: 45.00% 3.49 GB
// Percentage: 46.00% 3.55 GB
// Percentage: 47.00% 3.60 GB
// Percentage: 48.00% 3.66 GB
// Percentage: 49.00% 3.71 GB
// Percentage: 50.00% 3.76 GB
// Percentage: 51.00% 3.82 GB
// Percentage: 52.00% 3.87 GB
// Percentage: 53.00% 3.93 GB
// Percentage: 54.00% 3.98 GB
// Percentage: 55.00% 4.03 GB
// Percentage: 56.00% 4.09 GB
// Percentage: 57.00% 4.14 GB
// Percentage: 58.00% 4.20 GB
// Percentage: 59.00% 4.25 GB
// Percentage: 60.00% 4.30 GB
// Percentage: 61.00% 4.36 GB
// Percentage: 62.00% 4.41 GB
// Percentage: 63.00% 4.47 GB
// Percentage: 64.00% 4.52 GB
// Percentage: 65.00% 4.58 GB
// Percentage: 66.00% 4.64 GB
// Percentage: 67.00% 4.70 GB
// Percentage: 68.00% 4.77 GB
// Percentage: 69.00% 4.83 GB
// Percentage: 70.00% 4.89 GB
// Percentage: 71.00% 4.95 GB
// Percentage: 72.00% 5.01 GB
// Percentage: 73.00% 5.07 GB
// Percentage: 74.00% 5.14 GB
// Percentage: 75.00% 5.20 GB
// Percentage: 76.00% 5.26 GB
// Percentage: 77.00% 5.32 GB
// Percentage: 78.00% 5.38 GB
// Percentage: 79.00% 5.44 GB
// Percentage: 80.00% 5.50 GB
// Percentage: 81.00% 5.57 GB
// Percentage: 82.00% 5.63 GB
// Percentage: 83.00% 5.69 GB
// Percentage: 84.00% 5.75 GB
// Percentage: 85.00% 5.81 GB
// Percentage: 86.00% 5.87 GB
// Percentage: 87.00% 5.94 GB
// Percentage: 88.00% 6.00 GB
// Percentage: 89.00% 6.06 GB
// Percentage: 90.00% 6.12 GB
// Percentage: 91.00% 6.18 GB
// Percentage: 92.00% 6.24 GB
// Percentage: 93.00% 6.31 GB
// Percentage: 94.00% 6.37 GB
// Percentage: 95.00% 6.43 GB
// Percentage: 96.00% 6.49 GB
// Percentage: 97.00% 6.55 GB
// Percentage: 98.00% 6.61 GB
// Percentage: 99.00% 6.68 GB
// Total Ops.len=102000000
// 100M ops duration 171.214677875s
// Mem 6.74 GB
// Export snapshot duration 5.973242833s
// Mem 7.18 GB
// Import snapshot duration 843.4835ms
// New doc export snapshot time 5.637709334s
// Mem 9.33 GB
// Snapshot size 442.20 MB
// Mem usage after import snapshot 1.42 GB
// GetValue duration 2.074711167s
// Mem usage after getting value 2.12 GB
// ReExport Snapshot Duration 4.763463167s
// Export TrimmedSnapshot Duration 3.631642167s
// TrimmedSnapshot size 34.16 MB
// Import gc snapshot time: 3.068604667s
// Mem usage 1.14 GB
// GetValue duration 2.072992416s
// Mem usage after getting value 1.84 GB
