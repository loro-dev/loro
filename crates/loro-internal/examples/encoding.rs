use std::{io::Write, time::Instant};

use bench_utils::TextAction;
use flate2::write::GzEncoder;
use loro_internal::EncodeMode;
use loro_internal::LoroDoc;
use loro_internal::VersionVector;

fn main() {
    // let actions = bench_utils::get_automerge_actions();
    // let mut loro = LoroDoc::default();
    // let mut text = loro.get_text("text");
    // let mut txn = loro.txn().unwrap();

    // for TextAction { pos, ins, del } in actions.iter() {
    //     text.delete(&mut txn, *pos, *del).unwrap();
    //     text.insert(&mut txn, *pos, ins).unwrap();
    // }
    // drop(txn);

    // let start = Instant::now();
    // let buf = loro.export_from(&VersionVector::new());
    // println!(
    //     "encode changes {} bytes, used {}ms",
    //     buf.len(),
    //     start.elapsed().as_millis()
    // );
    // let json_ori = loro.to_json();
    // let start = Instant::now();
    // let buf_snapshot = loro.encode_all();
    // let _json_snapshot = loro.to_json();

    // println!(
    //     "encode snapshot {} bytes, used {}ms",
    //     buf_snapshot.len(),
    //     start.elapsed().as_millis()
    // );
    // let json_snapshot = loro.to_json();
    // let mut loro = LoroCore::default();
    // let start = Instant::now();
    // loro.decode(&buf).unwrap();
    // println!("decode rle_updates used {}ms", start.elapsed().as_millis());
    // let buf2 = loro.encode_with_cfg(EncodeMode::RleUpdates(VersionVector::new()));
    // assert_eq!(buf, buf2);
    // let json2 = loro.to_json();
    // assert_eq!(json_ori, json2);

    // let start = Instant::now();
    // let mut loro2 = LoroCore::default();
    // loro2.decode(&buf_snapshot).unwrap();
    // println!("decode snapshot used {}ms", start.elapsed().as_millis());
    // let json3 = loro2.to_json();
    // assert_eq!(json_snapshot, json3);

    // let start = Instant::now();
    // let update_buf = loro.encode_with_cfg(EncodeMode::Updates(VersionVector::new()));
    // println!(
    //     "encode updates {} bytes, used {}ms",
    //     update_buf.len(),
    //     start.elapsed().as_millis()
    // );
    // let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
    // encoder.write_all(&update_buf).unwrap();
    // let data = encoder.finish().unwrap();
    // println!("After compress updates have {} bytes", data.len());
    // let mut loro3 = LoroCore::default();
    // let start = Instant::now();
    // loro3.decode(&update_buf).unwrap();
    // println!("decode updates used {}ms", start.elapsed().as_millis());
    // let json_update = loro3.to_json();
    // assert_eq!(json_ori, json_update);
}
