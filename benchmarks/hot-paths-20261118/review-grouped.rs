use loro::{LoroDoc, UndoManager};
use std::time::Instant;

fn main() {
    let scenario = std::env::args().nth(1).unwrap();
    let count: usize = std::env::args().nth(2).unwrap().parse().unwrap();
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    let other = doc.get_text("other");
    let list = doc.get_list("list");
    let map = doc.get_map("map");
    if matches!(scenario.as_str(), "separated" | "delete") {
        text.insert(0, &"a".repeat(count)).unwrap();
    }
    if scenario == "map_tail" {
        map.insert("value", 0).unwrap();
    }
    doc.commit();
    let before_text = text.to_string();
    let before_map = map.get_value();
    let mut undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    undo.group_start().unwrap();
    let started = Instant::now();
    for index in 0..count {
        match scenario.as_str() {
            "contiguous" | "map_tail" => text.insert(index, "b").unwrap(),
            "separated" => text.insert(index * 2, "b").unwrap(),
            "delete" => text.delete(count - index - 1, 1).unwrap(),
            "two_texts" => {
                text.insert(index, "b").unwrap();
                other.insert(index, "c").unwrap();
            }
            "list" => list.push(index as i64).unwrap(),
            _ => panic!("unknown scenario"),
        }
        doc.commit();
    }
    if scenario == "map_tail" {
        map.insert("value", 1).unwrap();
        doc.commit();
    }
    undo.group_end();
    let edit_ns = started.elapsed().as_nanos();
    let after_text = text.to_string();
    let after_other = other.to_string();
    let after_list = list.get_value();
    let after_map = map.get_value();
    let started = Instant::now();
    assert!(undo.undo().unwrap());
    let undo_ns = started.elapsed().as_nanos();
    assert_eq!(text.to_string(), before_text);
    assert_eq!(other.to_string(), "");
    assert_eq!(list.len(), 0);
    assert_eq!(map.get_value(), before_map);
    let started = Instant::now();
    assert!(undo.redo().unwrap());
    let redo_ns = started.elapsed().as_nanos();
    assert_eq!(text.to_string(), after_text);
    assert_eq!(other.to_string(), after_other);
    assert_eq!(list.get_value(), after_list);
    assert_eq!(map.get_value(), after_map);
    println!("{scenario},{count},{edit_ns},{undo_ns},{redo_ns}");
}
