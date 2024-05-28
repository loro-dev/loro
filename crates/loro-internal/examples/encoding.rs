use std::time::Instant;

use bench_utils::TextAction;
use loro_internal::LoroDoc;

fn main() {
    let actions = bench_utils::get_automerge_actions();

    // let loro = LoroDoc::default();
    // let loro_b = LoroDoc::default();
    // let text = loro.get_text("text");
    // let mut i = 0;
    // let start = Instant::now();
    // for TextAction { pos, ins, del } in actions.iter() {
    //     {
    //         let mut txn = loro.txn().unwrap();
    //         text.delete(&mut txn, *pos, *del).unwrap();
    //         text.insert(&mut txn, *pos, ins).unwrap();
    //     }

    //     loro_b
    //         .import(&loro.export_from(&loro_b.oplog_vv()))
    //         .unwrap();
    //     i += 1;
    //     if i == 30000 {
    //         break;
    //     }
    // }

    // println!("{}ms", start.elapsed().as_millis());

    let loro = LoroDoc::default();
    let text = loro.get_text("text");

    for TextAction { pos, ins, del } in actions.iter() {
        let mut txn = loro.txn().unwrap();
        text.delete_with_txn(&mut txn, *pos, *del).unwrap();
        text.insert_with_txn(&mut txn, *pos, ins).unwrap();
        txn.commit().unwrap();
    }

    let start = Instant::now();
    let snapshot = loro.export_snapshot();
    println!("Snapshot time {}ms", start.elapsed().as_millis());
    let output = miniz_oxide::deflate::compress_to_vec(&snapshot, 6);
    println!(
        "Snapshot+compression time {}ms",
        start.elapsed().as_millis()
    );

    println!(
        "snapshot size {} after compression {}",
        snapshot.len(),
        output.len(),
    );

    let updates = loro.export_from(&Default::default());
    let output = miniz_oxide::deflate::compress_to_vec(&updates, 6);
    println!(
        "updates size {} after compression {}",
        updates.len(),
        output.len(),
    );

    let start = Instant::now();
    let blocks_bytes = loro.export_blocks();
    println!("blocks time {}ms", start.elapsed().as_millis());
    println!("blocks size {}", blocks_bytes.len());
    let blocks_bytes_compressed = miniz_oxide::deflate::compress_to_vec(&blocks_bytes, 6);
    println!(
        "blocks size after compression {}",
        blocks_bytes_compressed.len()
    );
    let start = Instant::now();
    let blocks_bytes = loro.export_blocks();
    println!("blocks time {}ms", start.elapsed().as_millis());
    println!("blocks size {}", blocks_bytes.len());
    let _blocks_bytes_compressed = miniz_oxide::deflate::compress_to_vec(&blocks_bytes, 6);
    println!(
        "blocks time after compression {}ms",
        start.elapsed().as_millis()
    );
    let start = Instant::now();
    let new_loro = LoroDoc::default();
    new_loro.import_blocks(&blocks_bytes).unwrap();
    println!("blocks decode time {:?}", start.elapsed());

    {
        // Delta encoding

        // let start = Instant::now();
        // for _ in 0..10 {
        //     loro.export_from(&Default::default());
        // }

        // println!("Avg encode {}ms", start.elapsed().as_millis() as f64 / 10.0);

        let data = loro.export_from(&Default::default());
        let start = Instant::now();
        for _ in 0..5 {
            let b = LoroDoc::default();
            b.detach();
            b.import(&data).unwrap();
        }

        println!("Avg decode {}ms", start.elapsed().as_millis() as f64 / 10.0);
        println!("size len={}", data.len());
        let d = miniz_oxide::deflate::compress_to_vec(&data, 10);
        println!("size after compress len={}", d.len());
    }

    // {
    //     // Snapshot encoding
    //     // println!("\n=======================\nSnapshot Encoding:");

    //     // let start = Instant::now();
    //     // for _ in 0..10 {
    //     //     loro.export_snapshot();
    //     // }

    //     // println!("Avg encode {}ms", start.elapsed().as_millis() as f64 / 10.0);

    //     // let data = loro.export_snapshot();
    //     // let start = Instant::now();
    //     // let times = 300;
    //     // for _ in 0..times {
    //     //     let b = LoroDoc::default();
    //     //     b.import(&data).unwrap();
    //     // }

    //     // println!(
    //     //     "Avg decode {}ms",
    //     //     start.elapsed().as_millis() as f64 / times as f64
    //     // );
    //     // println!("size len={}", data.len());
    //     // let d = miniz_oxide::deflate::compress_to_vec(&data, 10);
    //     // println!("size after compress len={}", d.len());
    // }
}
