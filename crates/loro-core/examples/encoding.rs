use std::io::Read;

use flate2::read::GzDecoder;
use loro_core::{container::registry::ContainerWrapper, LoroCore};
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
    let buf = loro.encode_snapshot();
    println!("{} bytes", buf.len());
}
