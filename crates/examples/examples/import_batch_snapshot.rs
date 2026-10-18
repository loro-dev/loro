//! Issue #1193: snapshot bootstrap of an editor-shaped document.
//! Run with `cargo run -p examples --release --example import_batch_snapshot`.
use loro::{ExportMode, LoroDoc, LoroList, LoroMap, LoroText};
use std::time::Instant;

fn main() {
    let source = LoroDoc::new();
    source.set_peer_id(1).unwrap();
    let blocks = source.get_list("blocks");
    let mut texts = Vec::new();
    for _ in 0..1_108 {
        let block = blocks.push_container(LoroMap::new()).unwrap();
        block.insert("nodeName", "paragraph").unwrap();
        block
            .insert_container("attributes", LoroMap::new())
            .unwrap();
        let children = block.insert_container("children", LoroList::new()).unwrap();
        for _ in 0..2 {
            let inline = children.push_container(LoroMap::new()).unwrap();
            inline.insert("nodeName", "text").unwrap();
            inline
                .insert_container("attributes", LoroMap::new())
                .unwrap();
            let content = inline
                .insert_container("children", LoroList::new())
                .unwrap();
            let text = content.push_container(LoroText::new()).unwrap();
            text.insert(0, "A rich-text paragraph with inline formatting and persistent collaborative history. ").unwrap();
            text.mark(0..12, "bold", true).unwrap();
            texts.push(text);
        }
    }
    source.commit();
    let snapshot = source.export(ExportMode::Snapshot).unwrap();
    let mut blobs = vec![snapshot];
    for i in 0..20 {
        let vv = source.oplog_vv();
        texts[i].insert(0, "edit ").unwrap();
        source.commit();
        blobs.push(source.export(ExportMode::updates(&vv)).unwrap());
    }
    println!(
        "1108 blocks, 12189 containers, snapshot {} bytes",
        blobs[0].len()
    );
    for updates in [1, 20] {
        for read in [false, true] {
            let mut samples = [Vec::new(), Vec::new()];
            for run in 0..14 {
                for side in [run % 2, 1 - run % 2] {
                    let doc = LoroDoc::new();
                    let t = Instant::now();
                    if side == 0 {
                        doc.import_batch(&blobs[..=updates]).unwrap();
                    } else {
                        doc.import(&blobs[0]).unwrap();
                        doc.import_batch(&blobs[1..=updates]).unwrap();
                    }
                    if read {
                        std::hint::black_box(doc.get_deep_value());
                    }
                    let elapsed = t.elapsed().as_secs_f64() * 1000.;
                    if run >= 2 {
                        samples[side].push(elapsed);
                    }
                    let expected = LoroDoc::new();
                    for blob in &blobs[..=updates] {
                        expected.import(blob).unwrap();
                    }
                    assert_eq!(doc.oplog_vv(), expected.oplog_vv());
                    assert_eq!(doc.state_frontiers(), expected.state_frontiers());
                    assert_eq!(doc.get_deep_value(), expected.get_deep_value());
                }
            }
            for sample in &mut samples {
                sample.sort_by(f64::total_cmp);
            }
            println!("+{updates:2} updates, full_read={read}: batch {:.3} ms; split {:.3} ms (median n=12)", samples[0][6], samples[1][6]);
        }
    }
}
