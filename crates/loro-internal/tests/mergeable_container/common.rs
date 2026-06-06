//! Shared helpers for the mergeable-container integration tests.
//!
//! Each split test file uses `#[path = "mergeable_container/common.rs"] mod common;` from
//! the harness in `tests/mergeable_container.rs`, so test fns can reach `common::doc` and
//! `common::sync` regardless of which sub-file they live in.

// Each split test file imports this module via `#[path = "common.rs"]`, so each helper appears
// as a separate copy per file. Suppress the dead-code warning that fires when an individual file
// only uses a subset of the helpers.
#![allow(dead_code)]

use loro_internal::{loro::ExportMode, LoroDoc};

pub fn doc(peer: u64) -> LoroDoc {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(peer).unwrap();
    doc
}

pub fn sync(a: &LoroDoc, b: &LoroDoc) {
    a.import(&b.export(ExportMode::updates(&a.oplog_vv())).unwrap())
        .unwrap();
    b.import(&a.export(ExportMode::updates(&b.oplog_vv())).unwrap())
        .unwrap();
}
