//! Importing ops on mergeable children whose causal dependencies (the child's creation
//! and/or its parent map) have not arrived yet must be buffered as pending, not panic.
//!
//! Regression for the `unreachable`/`Option::unwrap()` trap in `ContainerWrapper::new` when a
//! pending change referenced a mergeable container under a not-yet-imported parent map. Mergeable
//! cids live in the `ContainerID::Root` namespace, so the pending-root materialization path used
//! to treat them as ordinary roots and eagerly build their state — but a mergeable container is a
//! logical *child* whose depth cannot be resolved before its parent exists.

#[path = "common.rs"]
mod common;
use common::doc;

use loro_internal::{cursor::PosType, handler::MapHandler, loro::ExportMode};

#[test]
fn import_edit_on_mergeable_child_under_normal_map_before_creation() {
    // root map -> Normal child map `m` -> mergeable text `body` under `m`.
    // The mergeable child's parent is a Normal (non-root) container, so delivering the edit
    // without the creation exercises the unresolved-depth path that used to panic.
    let a = doc(1);
    let root = a.get_map("root");
    let m = root
        .insert_container("m", MapHandler::new_detached())
        .unwrap();
    a.commit_then_renew();
    let after_create = a.oplog_vv();

    // Capture the creation-only payload *now*, before the edit exists, so the second import
    // delivers strictly the missing dependency. If we exported it after the edit, it would also
    // re-include the edit and the convergence assert could pass even if the pending edit was lost.
    let creation_only = a.export(ExportMode::updates(&Default::default())).unwrap();

    let body = m.ensure_mergeable_text("body").unwrap();
    body.insert(0, "hi", PosType::Unicode).unwrap();
    a.commit_then_renew();

    // Only the edit commit, without the creation it causally depends on.
    let edit_only = a.export(ExportMode::updates(&after_create)).unwrap();

    let b = doc(2);
    // Must buffer as pending instead of panicking, and actually retain the edit as pending.
    let status = b.import(&edit_only).unwrap();
    assert!(
        status.pending.is_some(),
        "edit on a mergeable child whose creation is missing must be buffered as pending"
    );

    // Delivering the missing creation (which does NOT contain the edit) replays the pending
    // edit and converges.
    b.import(&creation_only).unwrap();
    assert_eq!(a.get_deep_value(), b.get_deep_value());
}
