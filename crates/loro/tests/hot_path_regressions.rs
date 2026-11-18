#[cfg(feature = "jsonpath")]
use loro::{Container, LoroMap, ValueOrContainer};
use loro::{LoroDoc, LoroText, UndoManager};
#[cfg(feature = "jsonpath")]
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Barrier,
};

#[test]
#[cfg(feature = "jsonpath")]
fn shared_child_handles_commit_once_when_the_last_two_drop_concurrently() {
    for _ in 0..200 {
        let doc = LoroDoc::new();
        let root = doc.get_map("root");
        root.insert_container("child", LoroMap::new()).unwrap();
        doc.commit();
        let first = match doc.jsonpath("$.root.*").unwrap().pop().unwrap() {
            ValueOrContainer::Container(Container::Map(map)) => map,
            _ => unreachable!(),
        };
        let second = first.clone();
        let updates = Arc::new(AtomicUsize::new(0));
        let observed = updates.clone();
        doc.subscribe_local_update(Box::new(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            true
        }))
        .detach();
        first.insert("value", 1).unwrap();
        drop(root);
        drop(doc);
        assert_eq!(updates.load(Ordering::SeqCst), 0);
        let barrier = Arc::new(Barrier::new(2));
        let other_barrier = barrier.clone();
        let worker = std::thread::spawn(move || {
            other_barrier.wait();
            drop(first);
        });
        barrier.wait();
        drop(second);
        worker.join().unwrap();
        assert_eq!(updates.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn grouped_text_inserts_preserve_creation_and_unicode_across_undo_redo() {
    let doc = LoroDoc::new();
    let root = doc.get_map("root");
    let mut undo = UndoManager::new(&doc);
    undo.group_start().unwrap();
    let text = root.insert_container("text", LoroText::new()).unwrap();
    doc.commit();
    for content in ["汉", "😀", "字", "e\u{301}"] {
        text.insert(text.len_unicode(), content).unwrap();
        doc.commit();
    }
    undo.group_end();
    let after = doc.get_deep_value();
    assert_eq!(text.to_string(), "汉😀字e\u{301}");
    assert!(undo.undo().unwrap());
    assert!(root.get("text").is_none());
    assert!(undo.redo().unwrap());
    assert_eq!(doc.get_deep_value(), after);
}

#[test]
fn inserted_runs_preserve_middle_insertions_and_intervening_deletions() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "outside").unwrap();
    doc.commit();
    let mut undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    text.insert(3, "汉😀").unwrap();
    text.insert(4, "abc").unwrap();
    text.insert(3, "前").unwrap();
    text.insert(9, "尾").unwrap();
    doc.commit();
    let after_insert = text.to_string();
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "outside");
    assert!(undo.redo().unwrap());
    assert_eq!(text.to_string(), after_insert);
    text.insert(0, "first").unwrap();
    text.delete(2, 2).unwrap();
    text.insert(2, "second").unwrap();
    doc.commit();
    let after_mixed = text.to_string();
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), after_insert);
    assert!(undo.redo().unwrap());
    assert_eq!(text.to_string(), after_mixed);
}

#[test]
fn cloning_handle_concurrently_during_another_handle_drop_preserves_final_commit() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };

    for _ in 0..100 {
        let doc = LoroDoc::new();
        let updates = Arc::new(AtomicUsize::new(0));
        let observed = updates.clone();
        doc.subscribe_local_update(Box::new(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            true
        }))
        .detach();

        let map1 = doc.get_map("m");
        let map2 = map1.clone();
        drop(doc);
        map1.insert("k1", 1).unwrap();

        let barrier = Arc::new(Barrier::new(2));
        let b2 = barrier.clone();
        let worker = std::thread::spawn(move || {
            b2.wait();
            let map3 = map2.clone();
            drop(map2);
            map3
        });
        barrier.wait();
        drop(map1);
        let surviving = worker.join().unwrap();
        assert_eq!(updates.load(Ordering::SeqCst), 0);

        surviving.insert("k2", 2).unwrap();
        drop(surviving);
        assert_eq!(updates.load(Ordering::SeqCst), 1);
    }
}
