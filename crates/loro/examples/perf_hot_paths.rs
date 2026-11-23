use loro::{ExportMode, LoroDoc, LoroList, LoroMap, UndoManager, VersionVector};
use std::{hint::black_box, time::Instant};

fn main() {
    let scenario = std::env::args().nth(1).expect("scenario");
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let mut undo = None;
    let mut updates = Vec::new();
    let mut maps = Vec::new();
    match scenario.as_str() {
        "map" | "map_large_batches" => {
            for index in 0..10_000 {
                let map = doc.get_map(format!("map-{index}"));
                map.insert("value", 0).unwrap();
                if scenario == "map_large_batches" {
                    maps.push(map);
                }
            }
            doc.commit();
        }
        "undo" | "undo_batched" | "undo_grouped" => {
            let mut manager = UndoManager::new(&doc);
            manager.set_merge_interval(0);
            if scenario == "undo_grouped" {
                manager.group_start().unwrap();
            }
            undo = Some(manager);
        }
        "root_lookup" | "child_lookup" => {
            doc.get_map("root")
                .insert_container("child", LoroMap::new())
                .unwrap();
            doc.commit();
        }
        "jsonpath" => {
            let books = doc
                .get_map("store")
                .insert_container("books", LoroList::new())
                .unwrap();
            for index in 0..2_000 {
                let book = books.insert_container(index, LoroMap::new()).unwrap();
                book.insert("title", format!("Book {index}")).unwrap();
                book.insert("price", index as i64).unwrap();
            }
            doc.commit();
        }
        "pending" => {
            let mut version = VersionVector::default();
            for index in 0..1_000 {
                doc.get_map("map").insert("value", index).unwrap();
                doc.commit();
                updates.push(doc.export(ExportMode::updates(&version)).unwrap());
                version = doc.oplog_vv();
            }
        }
        "text" => {}
        _ => panic!("unknown scenario"),
    }
    let started = Instant::now();
    match scenario.as_str() {
        "text" | "undo" | "undo_batched" | "undo_grouped" => {
            let text = doc.get_text("text");
            for index in 0..20_000 {
                text.insert(index, "a").unwrap();
                if scenario != "undo_batched" {
                    doc.commit();
                }
            }
            if scenario == "undo_batched" {
                doc.commit();
            }
            if scenario == "undo_grouped" {
                undo.as_mut().unwrap().group_end();
            }
        }
        "root_lookup" => {
            for _ in 0..100_000 {
                black_box(doc.get_map("root"));
            }
        }
        "child_lookup" => {
            let root = doc.get_map("root");
            for _ in 0..100_000 {
                black_box(root.get("child").unwrap());
            }
        }
        "map_large_batches" => {
            for value in 0..10 {
                for map in &maps {
                    map.insert("value", value).unwrap();
                }
                doc.commit();
            }
        }
        "map" => {
            let map = doc.get_map("map-0");
            for value in 0..2_000 {
                map.insert("value", value).unwrap();
                doc.commit();
            }
        }
        "jsonpath" => {
            #[cfg(feature = "jsonpath")]
            for _ in 0..1_000 {
                black_box(doc.jsonpath("$.store.books[*].title").unwrap());
            }
            #[cfg(not(feature = "jsonpath"))]
            panic!("jsonpath feature required");
        }
        "pending" => {
            for _ in 0..10 {
                let imported = LoroDoc::new();
                imported.detach();
                for update in updates.iter().rev() {
                    imported.import(update).unwrap();
                }
                assert_eq!(imported.oplog_vv(), doc.oplog_vv());
            }
        }
        _ => unreachable!(),
    }
    let elapsed_ns = started.elapsed().as_nanos();
    if matches!(
        scenario.as_str(),
        "text" | "undo" | "undo_batched" | "undo_grouped"
    ) {
        assert_eq!(doc.get_text("text").len_unicode(), 20_000);
    }
    if let Some(mut manager) = undo {
        assert!(manager.undo().unwrap());
        let expected = if scenario == "undo" { 19_999 } else { 0 };
        assert_eq!(doc.get_text("text").len_unicode(), expected);
        assert!(manager.redo().unwrap());
        assert_eq!(doc.get_text("text").len_unicode(), 20_000);
    }
    let restored = LoroDoc::new();
    restored
        .import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_eq!(restored.get_deep_value(), doc.get_deep_value());
    println!("{scenario},{elapsed_ns}");
}
