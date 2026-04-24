#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

mod import_atomicity;

use crate::{op::ListSlice, LoroDoc, LoroValue};

pub const PROPTEST_FACTOR_10: usize = 1;
pub const PROPTEST_FACTOR_1: usize = 0;

#[test]
fn size_of() {
    use crate::change::Change;
    use crate::{
        container::{map::MapSet, ContainerID},
        id::ID,
        op::{Op, RawOpContent},
        span::IdSpan,
        InternalString,
    };

    println!("Change {}", std::mem::size_of::<Change>());
    println!("Op {}", std::mem::size_of::<Op>());
    println!("InsertContent {}", std::mem::size_of::<RawOpContent>());
    println!("MapSet {}", std::mem::size_of::<MapSet>());
    println!("ListSlice {}", std::mem::size_of::<ListSlice>());
    println!("LoroValue {}", std::mem::size_of::<LoroValue>());
    println!("ID {}", std::mem::size_of::<ID>());
    println!("Vec {}", std::mem::size_of::<Vec<ID>>());
    println!("IdSpan {}", std::mem::size_of::<IdSpan>());
    println!("ContainerID {}", std::mem::size_of::<ContainerID>());
    println!("InternalString {}", std::mem::size_of::<InternalString>());
}

#[test]
#[should_panic(expected = "Locking order violation")]
fn reproduce_lock_violation_state_then_len_ops() {
    let doc = LoroDoc::new();
    let _state_guard = doc.app_state().lock(); // locks state(3)
    doc.len_ops(); // tries oplog(2) while state(3) held → PANIC!
}

#[test]
fn reproduce_no_violation_when_oplog_locked_first() {
    let doc = LoroDoc::new();
    let _oplog_guard = doc.oplog().lock(); // locks oplog(2) first
    let _state_guard = doc.app_state().lock(); // then state(3) — correct order
    let ops_count = doc.len_ops(); // should use cached value since oplog is locked
    assert_eq!(ops_count, 0);
    // when importing data, oplog is locked before state, so this path is safe
}

/// Simulates a realistic scenario: import holds both oplog and state,
/// then something within apply_diff triggers len_ops().
///
/// Since oplog IS locked during import's apply_diff, is_locked() should
/// detect it and len_ops() will use the cached value instead of re-locking.
#[test]
fn reproduce_import_scenario_oplog_already_held() {
    let doc = LoroDoc::new();
    // Mimic the import lock order: oplog first, then state
    let _oplog = doc.oplog().lock();
    let _state = doc.app_state().lock();
    // During apply_diff, this should use cache since oplog is locked
    let ops = doc.len_ops();
    assert_eq!(ops, 0); // empty doc, cache was initialized to 0
    let changes = doc.len_changes();
    assert_eq!(changes, 0);
}

/// Verify that the cache is stale after fresh import (no one updated it yet),
/// but len_ops() handles this correctly by locking oplog normally.
#[test]
fn reproduce_cache_refresh_on_import() {
    let doc = LoroDoc::new();
    let doc2 = LoroDoc::new();
    doc2.get_text("text").insert(0, "hi", crate::cursor::PosType::Bytes).unwrap();
    let snapshot = doc2.export(crate::encoding::ExportMode::snapshot()).unwrap();
    doc.import(&snapshot).unwrap();
    // After import, cache was refreshed before state lock, so it should be current
    let ops = doc.len_ops();
    assert!(ops > 0, "op count should be > 0 after import");
}
