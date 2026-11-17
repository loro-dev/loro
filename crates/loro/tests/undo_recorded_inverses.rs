//! Undo from inverses recorded at commit (context/undo-log.md) must behave as
//! the checkout it replaces, and what it keeps must count against
//! `max_undo_bytes` and the open transaction's bytes.

use loro::{LoroDoc, UndoManager};

const MIB: usize = 1024 * 1024;

/// A checkout sums a run's counter increments in op order, starting at 0;
/// float addition is not associative, so the inverse must too.
#[test]
fn counter_undo_sums_increments_in_op_order() {
    // One transaction.
    let doc = LoroDoc::new();
    let counter = doc.get_counter("c");
    doc.commit();
    let mut undo = UndoManager::new(&doc);
    for delta in [1e16, -1e16, 1.0] {
        counter.increment(delta).unwrap();
    }
    doc.commit();
    assert_eq!(counter.get_value(), 1.0);
    assert!(undo.undo().unwrap());
    assert_eq!(counter.get_value(), 0.0);
    assert!(undo.redo().unwrap());
    assert_eq!(counter.get_value(), 1.0);
    assert!(undo.undo().unwrap());
    assert_eq!(counter.get_value(), 0.0);

    // One undo item merged from several commits.
    let doc = LoroDoc::new();
    let counter = doc.get_counter("c");
    doc.commit();
    let mut undo = UndoManager::new(&doc);
    undo.group_start().unwrap();
    for delta in [1e16, -1e16, 1.0, 1e16, 2.0, -1e16] {
        counter.increment(delta).unwrap();
        doc.commit();
    }
    undo.group_end();
    let after = counter.get_value();
    assert_eq!(undo.undo_count(), 1);
    assert!(undo.undo().unwrap());
    assert_eq!(counter.get_value(), 0.0);
    assert!(undo.redo().unwrap());
    assert_eq!(counter.get_value(), after);
}

/// The values a recorded movable-list delete keeps (to restore the element)
/// count toward `max_undo_bytes`, so the item is evicted.
#[test]
fn recorded_movable_list_delete_counts_its_values() {
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    list.insert(0, "x".repeat(MIB)).unwrap();
    doc.commit();
    let mut undo = UndoManager::new(&doc);
    undo.set_max_undo_bytes(16 * 1024);
    list.delete(0, 1).unwrap();
    doc.commit();
    assert_eq!(undo.undo_count(), 0);
    assert!(doc.memory_stats().undo_bytes <= 16 * 1024);

    // Without a cap the item is kept and counted.
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    list.insert(0, "x".repeat(MIB)).unwrap();
    doc.commit();
    let undo = UndoManager::new(&doc);
    list.delete(0, 1).unwrap();
    doc.commit();
    assert_eq!(undo.undo_count(), 1);
    assert!(doc.memory_stats().undo_bytes >= MIB);
}

/// A recorded map write keeps the value it wrote (to tell whether undo changes
/// the key); that counts toward `max_undo_bytes` too.
#[test]
fn recorded_map_write_counts_the_value_it_keeps() {
    let doc = LoroDoc::new();
    let map = doc.get_map("m");
    let mut undo = UndoManager::new(&doc);
    undo.set_max_undo_bytes(16 * 1024);
    map.insert("k", "x".repeat(MIB)).unwrap();
    doc.commit();
    assert_eq!(undo.undo_count(), 0);
    assert!(doc.memory_stats().undo_bytes <= 16 * 1024);
}

/// Inverses recorded by an open transaction count as its uncommitted bytes.
#[test]
fn pending_recorded_inverses_count_as_uncommitted_bytes() {
    let doc = LoroDoc::new();
    let map = doc.get_map("m");
    map.insert("k", "x".repeat(MIB)).unwrap();
    doc.commit();
    let _undo = UndoManager::new(&doc);
    let before = doc.memory_stats().uncommitted_txn_bytes;
    map.insert("k", 1).unwrap();
    // The recorded inverse holds the 1 MiB value being overwritten.
    assert!(doc.memory_stats().uncommitted_txn_bytes >= before + MIB);
    doc.commit();
}
