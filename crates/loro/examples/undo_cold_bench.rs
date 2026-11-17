//! The first undo after opening a document: latency and the memory it leaves
//! behind (history caches, diff calculator, parsed history), compared with
//! the same edit without undo. Run with
//! `cargo run -p loro --release --example undo_cold_bench`.
use loro::{ExportMode, LoroDoc, TreeParentId, UndoManager};
use std::time::Instant;

fn build(kind: &str, size: usize) -> Vec<u8> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    match kind {
        "text" => {
            let t = doc.get_text("t");
            for i in 0..size / 100 {
                t.insert(t.len_unicode(), &format!("{:099} ", i)).unwrap();
                doc.commit();
            }
        }
        "tree" => {
            let tree = doc.get_tree("tr");
            let mut ids = Vec::new();
            for i in 0..size {
                let parent = if i < 10 {
                    TreeParentId::Root
                } else {
                    TreeParentId::Node(ids[i % ids.len()])
                };
                let id = tree.create(parent).unwrap();
                tree.get_meta(id).unwrap().insert("v", i as i64).unwrap();
                ids.push(id);
                if i % 100 == 99 {
                    doc.commit();
                }
            }
        }
        "maps" => {
            let root = doc.get_map("root");
            for i in 0..size {
                root.insert_container(&format!("c{i}"), loro::LoroMap::new())
                    .unwrap()
                    .insert("v", i as i64)
                    .unwrap();
                if i % 100 == 99 {
                    doc.commit();
                }
            }
        }
        _ => {
            let ml = doc.get_movable_list("ml");
            for i in 0..size {
                ml.push(i as i64).unwrap();
                if i % 100 == 99 {
                    doc.commit();
                }
            }
        }
    }
    doc.commit();
    doc.export(ExportMode::Snapshot).unwrap()
}

fn edit(kind: &str, doc: &LoroDoc) {
    match kind {
        "text" => {
            let t = doc.get_text("t");
            t.insert(t.len_unicode() / 2, "edit!").unwrap();
        }
        "tree" => {
            let tree = doc.get_tree("tr");
            let mut nodes = tree.nodes();
            nodes.sort();
            let id = tree
                .create(TreeParentId::Node(nodes[nodes.len() / 2]))
                .unwrap();
            tree.get_meta(id).unwrap().insert("v", -1).unwrap();
        }
        "maps" => {
            let child = doc
                .get_map("root")
                .get("c7")
                .unwrap()
                .into_container()
                .unwrap()
                .into_map()
                .unwrap();
            let next = child
                .get("v")
                .and_then(|v| v.into_value().ok())
                .map_or(0, |v| v.into_i64().unwrap_or(0));
            child.insert("v", next - 1).unwrap();
        }
        _ => {
            let ml = doc.get_movable_list("ml");
            ml.mov(1, ml.len() - 2).unwrap();
        }
    }
    doc.commit();
}

fn main() {
    println!("kind,size,first_undo_ms,second_undo_ms,total_bytes_before,total_bytes_after_undo,delta,history_cache_delta,diff_calc_delta,change_cache_delta,state_delta");
    for (kind, size) in [
        ("text", 1_000_000usize),
        ("tree", 10_000),
        ("tree", 40_000),
        ("maps", 10_000),
        ("movable_list", 100_000),
    ] {
        let snapshot = build(kind, size);
        let doc = LoroDoc::from_snapshot(&snapshot).unwrap();
        doc.set_peer_id(9).unwrap();
        let mut undo = UndoManager::new(&doc);
        edit(kind, &doc);
        edit(kind, &doc);
        let before = doc.memory_stats();
        let start = Instant::now();
        assert!(undo.undo().unwrap());
        let first = start.elapsed().as_secs_f64() * 1000.0;
        let after = doc.memory_stats();
        let start = Instant::now();
        assert!(undo.undo().unwrap());
        let second = start.elapsed().as_secs_f64() * 1000.0;
        let d = |a: usize, b: usize| b as i64 - a as i64;
        println!(
            "{kind},{size},{first:.2},{second:.2},{},{},{},{},{},{},{}",
            before.total_bytes,
            after.total_bytes,
            d(before.total_bytes, after.total_bytes),
            d(before.history_cache_bytes, after.history_cache_bytes),
            d(before.diff_calc_bytes, after.diff_calc_bytes),
            d(before.change_cache_bytes, after.change_cache_bytes),
            d(before.state_bytes, after.state_bytes),
        );
    }
}
