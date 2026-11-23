use loro::{ExportMode, LoroDoc, LoroMap, UndoManager};
use std::{hint::black_box, time::Instant};

fn main() {
    let scenario = std::env::args().nth(1).expect("scenario");
    let count: usize = std::env::args()
        .nth(2)
        .map(|value| value.parse().unwrap())
        .unwrap_or(20_000);
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    let root = doc.get_map("root");
    root.insert_container("child", LoroMap::new()).unwrap();
    doc.commit();
    let mut undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    if scenario == "grouped" {
        undo.group_start().unwrap();
    }
    let started = Instant::now();
    match scenario.as_str() {
        "root_lookup" => {
            for _ in 0..100_000 {
                black_box(doc.get_map("root"));
            }
        }
        "child_lookup" => {
            for _ in 0..100_000 {
                black_box(root.get("child").unwrap());
            }
        }
        "batched" | "grouped" => {
            for position in 0..count {
                text.insert(position, "a").unwrap();
                if scenario == "grouped" {
                    doc.commit();
                }
            }
            doc.commit();
            if scenario == "grouped" {
                undo.group_end();
            }
        }
        _ => panic!("unknown scenario"),
    }
    let edit_ns = started.elapsed().as_nanos();
    let mut undo_ns = 0;
    if matches!(scenario.as_str(), "batched" | "grouped") {
        let started = Instant::now();
        assert!(undo.undo().unwrap());
        undo_ns = started.elapsed().as_nanos();
        assert_eq!(text.len_unicode(), 0);
        assert!(undo.redo().unwrap());
        assert_eq!(text.len_unicode(), count);
    }
    let restored = LoroDoc::new();
    restored
        .import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_eq!(restored.get_deep_value(), doc.get_deep_value());
    println!("{scenario},{count},{edit_ns},{undo_ns}");
}
