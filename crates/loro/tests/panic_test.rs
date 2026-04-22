//! Tests that reproduce panic scenarios reachable through the public Rust API,
//! and tests that verify previously-panicking code paths now return errors.

#![allow(unexpected_cfgs)]
use serial_test::parallel;

use loro::event::{Diff, DiffBatch};
use loro::json::{JsonChange, JsonOp, JsonOpContent, JsonSchema, MapOp};
use loro::{CommitOptions, Container, ContainerID, ContainerType, LoroDoc, LoroList, ID};
use loro::{Frontiers, LoroValue};

// ---------------------------------------------------------------------------
// 1. Container getters panic on non-existent ContainerID (with clearer msg)
// ---------------------------------------------------------------------------

/// `LoroDoc::get_text` panics when given a `ContainerID` that does not exist in the document.
/// The panic message now explains the issue and points to `try_get_text` / `get_container`.
#[test]
#[parallel]
#[should_panic(
    expected = "The container does not exist in the document. Use `try_get_text` or `get_container` to check for existence."
)]
fn get_text_with_nonexistent_container_id_panics() {
    let doc = LoroDoc::new();
    let id = ContainerID::new_normal(ID::new(0, 0), ContainerType::Text);
    let _text = doc.get_text(id);
}

/// `LoroDoc::get_list` panics when given a `ContainerID` that does not exist in the document.
#[test]
#[parallel]
#[should_panic(
    expected = "The container does not exist in the document. Use `try_get_list` or `get_container` to check for existence."
)]
fn get_list_with_nonexistent_container_id_panics() {
    let doc = LoroDoc::new();
    let id = ContainerID::new_normal(ID::new(0, 0), ContainerType::List);
    let _list = doc.get_list(id);
}

/// `LoroDoc::get_map` panics when given a `ContainerID` that does not exist in the document.
#[test]
#[parallel]
#[should_panic(
    expected = "The container does not exist in the document. Use `try_get_map` or `get_container` to check for existence."
)]
fn get_map_with_nonexistent_container_id_panics() {
    let doc = LoroDoc::new();
    let id = ContainerID::new_normal(ID::new(0, 0), ContainerType::Map);
    let _map = doc.get_map(id);
}

/// `LoroDoc::get_tree` panics when given a `ContainerID` that does not exist in the document.
#[test]
#[parallel]
#[should_panic(
    expected = "The container does not exist in the document. Use `try_get_tree` or `get_container` to check for existence."
)]
fn get_tree_with_nonexistent_container_id_panics() {
    let doc = LoroDoc::new();
    let id = ContainerID::new_normal(ID::new(0, 0), ContainerType::Tree);
    let _tree = doc.get_tree(id);
}

// ---------------------------------------------------------------------------
// 1b. try_get_* returns None for non-existent containers (safe alternative)
// ---------------------------------------------------------------------------

#[test]
#[parallel]
fn try_get_text_returns_none_for_missing_container() {
    let doc = LoroDoc::new();
    let id = ContainerID::new_normal(ID::new(0, 0), ContainerType::Text);
    assert!(doc.try_get_text(id).is_none());
}

#[test]
#[parallel]
fn try_get_list_returns_none_for_missing_container() {
    let doc = LoroDoc::new();
    let id = ContainerID::new_normal(ID::new(0, 0), ContainerType::List);
    assert!(doc.try_get_list(id).is_none());
}

#[test]
#[parallel]
fn try_get_map_returns_none_for_missing_container() {
    let doc = LoroDoc::new();
    let id = ContainerID::new_normal(ID::new(0, 0), ContainerType::Map);
    assert!(doc.try_get_map(id).is_none());
}

#[test]
#[parallel]
fn try_get_tree_returns_none_for_missing_container() {
    let doc = LoroDoc::new();
    let id = ContainerID::new_normal(ID::new(0, 0), ContainerType::Tree);
    assert!(doc.try_get_tree(id).is_none());
}

// ---------------------------------------------------------------------------
// 2. Detached container operations — FIXED: now return Err instead of panicking
// ---------------------------------------------------------------------------

/// A detached `LoroList::insert` used to panic when `pos` > list length.
/// It now returns `LoroError::OutOfBound`.
#[test]
#[parallel]
fn detached_list_insert_out_of_bounds_returns_error() {
    let list = LoroList::new();
    let err = list.insert(10, "x").unwrap_err();
    assert!(matches!(err, loro::LoroError::OutOfBound { .. }));
}

// ---------------------------------------------------------------------------
// 3. Nested transaction — FIXED in main: now returns Err instead of panicking
// ---------------------------------------------------------------------------

/// `txn()` used to panic when another transaction was already active.
/// After the merge, it returns `Err(LoroError::DuplicatedTransactionError)`.
#[test]
#[parallel]
fn nested_transaction_now_returns_error() {
    let doc = LoroDoc::new();
    let err = doc.inner().txn().unwrap_err();
    assert!(matches!(err, loro::LoroError::DuplicatedTransactionError));
}

// ---------------------------------------------------------------------------
// 4. commit_with immediate_renew on a detached (non-editable) document — FIXED
// ---------------------------------------------------------------------------

/// `LoroDoc::commit_with` with `immediate_renew(true)` used to panic when the
/// document was detached and detached editing was disabled after the auto-commit
/// transaction had already been renewed.
///
/// It now silently skips the renew (no panic, no error).
#[test]
#[parallel]
fn commit_with_immediate_renew_on_detached_doc_no_longer_panics() {
    let doc = LoroDoc::new();
    doc.get_text("text").insert(0, "hello").unwrap();
    doc.set_detached_editing(true);
    doc.detach();
    doc.set_detached_editing(false);
    doc.commit_with(CommitOptions::new().immediate_renew(true));
}

// ---------------------------------------------------------------------------
// 5. Tree mov_after / mov_before with a deleted node — FIXED
// ---------------------------------------------------------------------------

/// `LoroTree::mov_after` used to panic when the `other` node had been deleted.
/// It now returns `LoroTreeError::TreeNodeDeletedOrNotExist`.
#[test]
#[parallel]
fn tree_mov_after_deleted_node_returns_error() {
    let doc = LoroDoc::new();
    let tree = doc.get_tree("root");
    let a = tree.create(None).unwrap();
    let b = tree.create(None).unwrap();
    tree.delete(b).unwrap();
    let err = tree.mov_after(a, b).unwrap_err();
    assert!(matches!(
        err,
        loro::LoroError::TreeError(loro::LoroTreeError::TreeNodeDeletedOrNotExist(_))
    ));
}

/// `LoroTree::mov_before` used to panic when the `other` node had been deleted.
/// Same root cause as `mov_after`.
#[test]
#[parallel]
fn tree_mov_before_deleted_node_returns_error() {
    let doc = LoroDoc::new();
    let tree = doc.get_tree("root");
    let a = tree.create(None).unwrap();
    let b = tree.create(None).unwrap();
    tree.delete(b).unwrap();
    let err = tree.mov_before(a, b).unwrap_err();
    assert!(matches!(
        err,
        loro::LoroError::TreeError(loro::LoroTreeError::TreeNodeDeletedOrNotExist(_))
    ));
}

// ---------------------------------------------------------------------------
// 6. Container::new with ContainerType::Unknown
// ---------------------------------------------------------------------------

/// `Container::new(ContainerType::Unknown(_))` hits an `unreachable!()` arm.
/// The panic message is now explicit.
#[test]
#[parallel]
#[should_panic(expected = "Cannot create a detached container of type Unknown")]
fn container_new_unknown_panics() {
    let _container = Container::new(ContainerType::Unknown(0));
}

// ---------------------------------------------------------------------------
// 7. apply_diff with mismatched diff type — FIXED
// ---------------------------------------------------------------------------

/// `LoroDoc::apply_diff` used to panic when the diff type didn't match the
/// target container's type (e.g. a `Text` diff sent to a `Map`).
/// It now returns `LoroError::DecodeError`.
#[test]
#[parallel]
fn apply_diff_with_wrong_type_returns_error() {
    let doc = LoroDoc::new();
    let mut batch = DiffBatch::default();
    let map_id = ContainerID::new_root("map", ContainerType::Map);
    // Push a text diff for a map container – type mismatch
    batch.push(map_id, Diff::Text(vec![])).unwrap();
    let err = doc.apply_diff(batch).unwrap_err();
    assert!(matches!(err, loro::LoroError::DecodeError(..)));
}

// ---------------------------------------------------------------------------
// 8. import_json_updates with malformed peer index — FIXED
// ---------------------------------------------------------------------------

/// `import_json_updates` used to panic when the `peers` array was shorter than
/// a peer index referenced in a change. It now falls back to the raw peer id
/// instead of panicking.
#[test]
#[parallel]
fn import_json_updates_with_short_peers_array_no_longer_panics() {
    let doc = LoroDoc::new();
    let schema = JsonSchema {
        schema_version: 1,
        start_version: Frontiers::default(),
        peers: Some(vec![1u64]),
        changes: vec![JsonChange {
            id: ID::new(5u64, 0),
            timestamp: 0,
            deps: vec![],
            lamport: 0,
            msg: None,
            ops: vec![JsonOp {
                content: JsonOpContent::Map(MapOp::Insert {
                    key: "x".into(),
                    value: LoroValue::Null,
                }),
                container: ContainerID::new_root("map", ContainerType::Map),
                counter: 0,
            }],
        }],
    };
    // The import may fail for other reasons (unknown peer, missing deps, etc.)
    // but it must NOT panic on the out-of-bounds peer index.
    let _ = doc.import_json_updates(schema);
}

// ---------------------------------------------------------------------------
// 9. Detached tree methods that used to panic — FIXED
// ---------------------------------------------------------------------------

use loro::LoroTree;

/// `LoroTree::is_fractional_index_enabled` used to panic on a detached tree.
#[test]
#[parallel]
fn detached_tree_is_fractional_index_enabled_reports_enabled() {
    let tree = LoroTree::new();
    assert!(tree.is_fractional_index_enabled());
}

/// `LoroTree::enable_fractional_index` / `disable_fractional_index`
/// used to panic on a detached tree.
#[test]
#[parallel]
fn detached_tree_enable_disable_fractional_index_does_not_panic() {
    let tree = LoroTree::new();
    tree.enable_fractional_index(1);
    tree.disable_fractional_index();
}
