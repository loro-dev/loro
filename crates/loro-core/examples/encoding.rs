use std::{io::Write, time::Instant};

use bench_utils::TextAction;
use flate2::write::GzEncoder;
use loro_core::{configure::Configure, container::registry::ContainerWrapper, LoroCore};

fn main() {
    let actions = bench_utils::get_automerge_actions();
    let mut loro = LoroCore::default();
    let text = loro.get_text("text");
    text.with_container(|text| {
        for TextAction { pos, ins, del } in actions.iter() {
            text.delete(&loro, *pos, *del);
            text.insert(&loro, *pos, ins);
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
