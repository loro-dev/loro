use std::{
    io::{Read, Write},
    time::Instant,
};

use flate2::{read::GzDecoder, write::GzEncoder};
use loro_core::{configure::Configure, container::registry::ContainerWrapper, LoroCore};
use serde_json::Value;
const RAW_DATA: &[u8; 901823] = include_bytes!("../benches/automerge-paper.json.gz");

fn main() {
    let mut d = GzDecoder::new(&RAW_DATA[..]);
    let mut s = String::new();
    d.read_to_string(&mut s).unwrap();
    let json: Value = serde_json::from_str(&s).unwrap();
    let txns = json.as_object().unwrap().get("txns");
    let mut loro = LoroCore::default();
    let text = loro.get_text("text");
    text.with_container(|text| {
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
                text.delete(&loro, pos, del_here);
                text.insert(&loro, pos, ins_content);
            }
        }
    });
    let start = Instant::now();
    let buf = loro.encode_snapshot();
    println!(
        "{} bytes, overhead {} bytes. used {}ms",
        buf.len(),
        0,
        start.elapsed().as_millis()
    );
    let json1 = loro.to_json();
    let start = Instant::now();
    let loro = LoroCore::decode_snapshot(&buf, None, Configure::default());
    println!("decode used {}ms", start.elapsed().as_millis());
    let buf2 = loro.encode_snapshot();
    assert_eq!(buf, buf2);
    let json2 = loro.to_json();
    assert_eq!(json1, json2);
    let update_buf = loro.export_updates(&Default::default()).unwrap();
    println!("Updates have {} bytes", update_buf.len());
    let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&update_buf).unwrap();
    let data = encoder.finish().unwrap();
    println!("After compress updates have {} bytes", data.len());
}
