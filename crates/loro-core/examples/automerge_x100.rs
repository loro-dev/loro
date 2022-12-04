#[cfg(not(feature = "test_utils"))]
fn main() {}

#[cfg(feature = "test_utils")]
fn main() {
    const RAW_DATA: &[u8; 901823] = include_bytes!("../benches/automerge-paper.json.gz");
    use std::{io::Read, time::Instant};

    use flate2::read::GzDecoder;
    use loro_core::LoroCore;
    use serde_json::Value;

    let mut d = GzDecoder::new(&RAW_DATA[..]);
    let mut s = String::new();
    d.read_to_string(&mut s).unwrap();
    let json: Value = serde_json::from_str(&s).unwrap();
    let txns = json.as_object().unwrap().get("txns");
    println!("Txn: {}", txns.unwrap().as_array().unwrap().len());

    let mut loro = LoroCore::default();
    let start = Instant::now();
    for _ in 0..100 {
        for (_, txn) in txns.unwrap().as_array().unwrap().iter().enumerate() {
            let mut text = loro.get_text("text");
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
                text.delete(&loro, pos, del_here).unwrap();
                text.insert(&loro, pos, ins_content).unwrap();
            }
        }
    }
    println!("{}", start.elapsed().as_millis());
}
