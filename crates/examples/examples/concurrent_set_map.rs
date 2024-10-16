use std::time::Instant;

use loro::{LoroDoc, LoroResult};

fn main() -> LoroResult<()> {
    const N: usize = 100_000;
    let mut updates = vec![];
    let mut docs = vec![];
    for _ in 0..N {
        docs.push(LoroDoc::new());
    }

    println!("Applied. Start exporting.");
    println!("Apply set map");

    for (i, doc) in docs.iter().enumerate() {
        doc.get_map("map").insert("v", i as i32)?;
        updates.push(doc.export(loro::ExportMode::all_updates()).unwrap());
    }
    // for update in updates.iter() {
    //     docs[0].import(update)?;
    // }
    println!("Start import other updates");
    let s = Instant::now();
    docs[0].import_batch(&updates).unwrap();
    println!("Concurrently set Map {} ms", s.elapsed().as_millis());

    Ok(())
}
