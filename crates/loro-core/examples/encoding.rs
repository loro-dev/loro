use std::{io::Write, time::Instant};

use bench_utils::TextAction;
use flate2::write::GzEncoder;
use loro_core::VersionVector;
use loro_core::{container::registry::ContainerWrapper, LoroCore};
const RAW_DATA: &[u8; 901823] = include_bytes!("../benches/automerge-paper.json.gz");

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
    let buf = loro.encode_changes(&VersionVector::new(), false);
    let json1 = loro.to_json();

    println!(
        "encode changes {} bytes, used {}ms",
        buf.len(),
        start.elapsed().as_millis()
    );
    let start = Instant::now();
    let buf_snapshot = loro.encode_snapshot(false);
    let json_snapshot = loro.to_json();

    println!(
        "encode snapshot {} bytes, used {}ms",
        buf_snapshot.len(),
        start.elapsed().as_millis()
    );
    let mut loro = LoroCore::default();
    let start = Instant::now();
    loro.decode_changes(&buf);
    println!("decode changes used {}ms", start.elapsed().as_millis());
    let buf2 = loro.encode_changes(&VersionVector::new(), false);
    assert_eq!(buf, buf2);
    let json2 = loro.to_json();
    assert_eq!(json1, json2);

    let start = Instant::now();
    let loro2 = LoroCore::decode_snapshot(&buf_snapshot, Default::default(), None);
    println!("decode snapshot used {}ms", start.elapsed().as_millis());
    let json3 = loro2.to_json();
    assert_eq!(json_snapshot, json3);

    let update_buf = loro.export_updates(&Default::default()).unwrap();
    println!("Updates have {} bytes", update_buf.len());
    let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&update_buf).unwrap();
    let data = encoder.finish().unwrap();
    println!("After compress updates have {} bytes", data.len());
}
