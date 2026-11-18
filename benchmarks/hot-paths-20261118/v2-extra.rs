use loro::{
    ExportMode, LoroDoc, LoroList, LoroMap, LoroValue, MemoryBudget, UndoManager, ValueOrContainer,
};
use std::{hint::black_box, time::Instant};

fn main() {
    let scenario = std::env::args().nth(1).expect("scenario");
    match scenario.as_str() {
        "undo_budget" => {
            let doc = LoroDoc::new();
            doc.set_peer_id(1).unwrap();
            doc.set_memory_budget(MemoryBudget::new().with_max_undo_bytes(1 << 22));
            let mut undo = UndoManager::new(&doc);
            undo.set_max_undo_steps(10_000);
            undo.set_merge_interval(0);
            let text = doc.get_text("text");
            let started = Instant::now();
            for index in 0..5_000 {
                text.insert(index, "a").unwrap();
                doc.commit();
            }
            let elapsed_ns = started.elapsed().as_nanos();
            assert_eq!(undo.undo_count(), 5_000);
            assert!(undo.undo().unwrap());
            println!("{scenario},{elapsed_ns}");
        }
        "snapshot_map_get" => {
            let src = LoroDoc::new();
            src.set_peer_id(1).unwrap();
            let src_map = src.get_map("big");
            for i in 0..2_000 {
                src_map
                    .insert(&format!("k{i}"), format!("value-{i}"))
                    .unwrap();
            }
            src.commit();
            let snapshot = src.export(ExportMode::Snapshot).unwrap();
            let doc = LoroDoc::new();
            doc.import(&snapshot).unwrap();
            let map = doc.get_map("big");
            assert!(map.get("k0").is_some());
            let started = Instant::now();
            for _ in 0..50_000 {
                black_box(map.get("k1000").unwrap());
            }
            let elapsed_ns = started.elapsed().as_nanos();
            println!("{scenario},{elapsed_ns}");
        }
        "snapshot_jsonpath" => {
            let src = LoroDoc::new();
            src.set_peer_id(1).unwrap();
            let books = src
                .get_map("store")
                .insert_container("books", LoroList::new())
                .unwrap();
            for index in 0..2_000 {
                let book = books.insert_container(index, LoroMap::new()).unwrap();
                book.insert("title", format!("Book {index}")).unwrap();
                book.insert("price", index as i64).unwrap();
            }
            src.commit();
            let snapshot = src.export(ExportMode::Snapshot).unwrap();
            let doc = LoroDoc::new();
            doc.import(&snapshot).unwrap();
            assert_eq!(doc.jsonpath("$.store.books[*].title").unwrap().len(), 2_000);
            let started = Instant::now();
            for _ in 0..500 {
                black_box(doc.jsonpath("$.store.books[*].title").unwrap());
            }
            let elapsed_ns = started.elapsed().as_nanos();
            println!("{scenario},{elapsed_ns}");
        }
        "map_for_each_scalar" => {
            let doc = LoroDoc::new();
            doc.set_peer_id(1).unwrap();
            let map = doc.get_map("m");
            for i in 0..8 {
                map.insert(&format!("k{i}"), i as i64).unwrap();
            }
            doc.commit();
            let mut visits = 0usize;
            let started = Instant::now();
            for _ in 0..100_000 {
                map.for_each(|_, v| {
                    assert!(matches!(
                        black_box(v),
                        ValueOrContainer::Value(LoroValue::I64(_))
                    ));
                    visits += 1;
                });
            }
            let elapsed_ns = started.elapsed().as_nanos();
            assert_eq!(visits, 800_000);
            println!("{scenario},{elapsed_ns}");
        }
        _ => panic!("unknown scenario {scenario}"),
    }
}
