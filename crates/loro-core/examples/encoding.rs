use std::{io::Write, time::Instant};

use bench_utils::TextAction;
use flate2::write::GzEncoder;
use loro_core::VersionVector;
use loro_core::{
    container::registry::ContainerWrapper,
    log_store::{EncodeConfig, EncodeMode},
    LoroCore,
};

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
    let buf = loro
        .encode(EncodeConfig::new(
            EncodeMode::RleUpdates(VersionVector::new()),
            None,
        ))
        .unwrap();
    let json_ori = loro.to_json();

    println!(
        "encode changes {} bytes, used {}ms",
        buf.len(),
        start.elapsed().as_millis()
    );
    let start = Instant::now();
    let buf_snapshot = loro.encode(EncodeConfig::from_vv(None)).unwrap();
    let json_snapshot = loro.to_json();

    println!(
        "encode snapshot {} bytes, used {}ms",
        buf_snapshot.len(),
        start.elapsed().as_millis()
    );
    let mut loro = LoroCore::default();
    let start = Instant::now();
    loro.decode(&buf).unwrap();
    println!("decode rle_updates used {}ms", start.elapsed().as_millis());
    let buf2 = loro
        .encode(EncodeConfig::new(
            EncodeMode::RleUpdates(VersionVector::new()),
            None,
        ))
        .unwrap();
    assert_eq!(buf, buf2);
    let json2 = loro.to_json();
    assert_eq!(json_ori, json2);

    let start = Instant::now();
    let mut loro2 = LoroCore::default();
    loro2.decode(&buf_snapshot).unwrap();
    println!("decode snapshot used {}ms", start.elapsed().as_millis());
    let json3 = loro2.to_json();
    assert_eq!(json_snapshot, json3);

    let start = Instant::now();
    let update_buf = loro
        .encode(EncodeConfig::new(
            EncodeMode::Updates(VersionVector::new()),
            None,
        ))
        .unwrap();
    println!("encode updates {} bytes, used {}ms", update_buf.len(), start.elapsed().as_millis());
    let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&update_buf).unwrap();
    let data = encoder.finish().unwrap();
    println!("After compress updates have {} bytes", data.len());
    let mut loro3 = LoroCore::default();
    let start = Instant::now();
    loro3.decode(&update_buf).unwrap();
    println!("decode updates used {}ms", start.elapsed().as_millis());
    let json_update = loro3.to_json();
    assert_eq!(json_ori, json_update);
}
