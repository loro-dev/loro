use dev_utils::ByteSize;
use loro::LoroDoc;
use std::time::Instant;

pub fn bench_fast_snapshot(doc: &LoroDoc) {
    {
        println!("======== Old snapshot mode =========");
        let start = Instant::now();
        let snapshot = doc.export_snapshot();
        let elapsed = start.elapsed();
        println!("Snapshot size: {}", ByteSize(snapshot.len()));
        println!("Export snapshot time: {:?}", elapsed);

        let start = Instant::now();
        let doc = LoroDoc::new();
        doc.import(&snapshot).unwrap();
        let elapsed = start.elapsed();
        println!("Import snapshot time: {:?}", elapsed);
    }

    {
        println!("======== New snapshot mode =========");
        let start = Instant::now();
        let snapshot = doc.export_fast_snapshot();
        let elapsed = start.elapsed();
        println!("Fast Snapshot size: {}", ByteSize(snapshot.len()));
        println!("Export fast snapshot time: {:?}", elapsed);

        let mem = dev_utils::get_mem_usage();
        let start = Instant::now();
        let new_doc = LoroDoc::new();
        new_doc.import(&snapshot).unwrap();
        let elapsed = start.elapsed();
        println!("Import fast snapshot time: {:?}", elapsed);
        println!(
            "Memory usage for new doc: {}",
            dev_utils::get_mem_usage() - mem
        );
        assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());
        println!(
            "Memory usage for new doc after getting deep value: {}",
            dev_utils::get_mem_usage() - mem
        );
        new_doc.check_state_correctness_slow();
    }
}
