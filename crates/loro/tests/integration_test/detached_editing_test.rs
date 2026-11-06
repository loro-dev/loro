use pretty_assertions::assert_eq;
use std::sync::{Arc, Mutex};

use loro::{ExportMode, Frontiers, LoroDoc, LoroError, UndoManager, UpdateTimeoutError, ID};
use loro_internal::vv;

#[test]
fn disallow_editing_on_detached_mode_by_default() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.get_text("text").insert(0, "Hello world!").unwrap();
    doc.checkout(&Frontiers::from(ID::new(1, 5))).unwrap();
    match doc.get_text("text").insert(0, "h") {
        Err(LoroError::EditWhenDetached) => {}
        Err(LoroError::AutoCommitNotStarted) => {}
        Ok(_) => panic!(""),
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn allow_editing_on_detached_mode_when_detached_editing_is_enabled() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let string = Arc::new(Mutex::new(String::new()));
    let string_clone = string.clone();
    let _g = doc.subscribe_root(Arc::new(move |batch| {
        for e in batch.events {
            match e.diff {
                loro::event::Diff::Text(vec) => {
                    let mut s = string_clone.lock().unwrap();
                    let mut index = 0;
                    for op in vec {
                        match op {
                            loro::TextDelta::Retain { retain, .. } => {
                                index += retain;
                            }
                            loro::TextDelta::Insert { insert, .. } => {
                                s.replace_range(index..index, &insert);
                                index += insert.len();
                            }
                            loro::TextDelta::Delete { delete } => {
                                s.replace_range(index..index + delete, "");
                            }
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
    }));
    doc.set_detached_editing(true);
    doc.get_text("text").insert(0, "Hello world!").unwrap();
    doc.checkout(&Frontiers::from(ID::new(1, 4))).unwrap();
    doc.set_detached_editing(true);
    assert_ne!(
        1,
        doc.peer_id(),
        "peer id should be changed after checkout in detached editing mode"
    );
    doc.set_peer_id(2).unwrap();
    doc.get_text("text").insert(5, " alice!").unwrap();
    doc.commit();
    assert_eq!(doc.get_text("text").to_string(), "Hello alice!");
    assert_eq!(&**string.lock().unwrap(), "Hello alice!");
    assert_ne!(doc.state_frontiers(), doc.oplog_frontiers());
    assert_ne!(doc.oplog_vv(), doc.state_vv());
    assert_eq!(doc.state_frontiers(), Frontiers::from(ID::new(2, 6)));
    assert_eq!(
        doc.oplog_frontiers(),
        Frontiers::from([ID::new(1, 11), ID::new(2, 6)])
    );
    assert_eq!(doc.state_vv(), vv!(1 => 5, 2 => 7));
    assert_eq!(doc.oplog_vv(), vv!(1 => 12, 2 => 7));

    doc.checkout_to_latest();
    assert_ne!(
        2,
        doc.peer_id(),
        "peer id should be changed after checkout in detached editing mode"
    );
    assert_eq!(doc.get_text("text").to_string(), "Hello world! alice!");
    assert_eq!(&**string.lock().unwrap(), "Hello world! alice!");
    assert_eq!(doc.state_frontiers(), doc.oplog_frontiers());
    assert_eq!(doc.oplog_vv(), doc.state_vv());

    // New op on peer id 1
    doc.set_peer_id(1).unwrap();
    doc.get_text("text").insert(0, "Hi ").unwrap();
    doc.commit();
    assert_eq!(&**string.lock().unwrap(), "Hi Hello world! alice!");
    assert_eq!(doc.get_text("text").to_string(), "Hi Hello world! alice!");
    assert_eq!(doc.state_frontiers(), doc.oplog_frontiers());
    assert_eq!(doc.oplog_vv(), doc.state_vv());
    assert_eq!(doc.state_frontiers(), Frontiers::from_id(ID::new(1, 14)));
    let json = doc
        .export_json_updates(&Default::default(), &doc.oplog_vv())
        .unwrap();
    let actual = format!("{json:#?}");
    let expected = r#"JsonSchema {
    schema_version: 1,
    start_version: Frontiers(
        [],
    ),
    peers: Some(
        [
            1,
            2,
        ],
    ),
    changes: [
        JsonChange {
            id: 0@0,
            timestamp: 0,
            deps: [],
            lamport: 0,
            msg: None,
            ops: [
                JsonOp {
                    content: Text(
                        Insert {
                            pos: 0,
                            text: "Hello world!",
                        },
                    ),
                    container: Root("text" Text),
                    counter: 0,
                },
            ],
        },
        JsonChange {
            id: 0@1,
            timestamp: 0,
            deps: [
                4@0,
            ],
            lamport: 5,
            msg: None,
            ops: [
                JsonOp {
                    content: Text(
                        Insert {
                            pos: 5,
                            text: " alice!",
                        },
                    ),
                    container: Root("text" Text),
                    counter: 0,
                },
            ],
        },
        JsonChange {
            id: 12@0,
            timestamp: 0,
            deps: [
                11@0,
                6@1,
            ],
            lamport: 12,
            msg: None,
            ops: [
                JsonOp {
                    content: Text(
                        Insert {
                            pos: 0,
                            text: "Hi ",
                        },
                    ),
                    container: Root("text" Text),
                    counter: 12,
                },
            ],
        },
    ],
}"#;
    assert_eq!(&actual, expected);
}

#[test]
fn allow_editing_on_detached_mode_when_detached_editing_is_enabled_2() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let string = Arc::new(Mutex::new(String::new()));
    let string_clone = string.clone();
    let _g = doc.subscribe_root(Arc::new(move |batch| {
        for e in batch.events {
            match e.diff {
                loro::event::Diff::Text(vec) => {
                    let mut s = string_clone.lock().unwrap();
                    let mut index = 0;
                    for op in vec {
                        match op {
                            loro::TextDelta::Retain { retain, .. } => {
                                index += retain;
                            }
                            loro::TextDelta::Insert { insert, .. } => {
                                s.replace_range(index..index, &insert);
                                index += insert.len();
                            }
                            loro::TextDelta::Delete { delete } => {
                                s.replace_range(index..index + delete, "");
                            }
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
    }));
    doc.set_detached_editing(true);
    doc.get_text("text").insert(0, "Hello world!").unwrap();
    doc.set_detached_editing(true);
    doc.checkout(&Frontiers::from(ID::new(1, 4))).unwrap();
    assert_ne!(
        1,
        doc.peer_id(),
        "peer id should be changed after checkout in detached editing mode"
    );
    doc.set_peer_id(0).unwrap();
    doc.get_text("text").insert(5, " alice!").unwrap();
    doc.commit();
    assert_eq!(doc.get_text("text").to_string(), "Hello alice!");
    assert_eq!(&**string.lock().unwrap(), "Hello alice!");
    assert_ne!(doc.state_frontiers(), doc.oplog_frontiers());
    assert_ne!(doc.oplog_vv(), doc.state_vv());
    assert_eq!(doc.state_frontiers(), Frontiers::from(ID::new(0, 6)));
    assert_eq!(
        doc.oplog_frontiers(),
        Frontiers::from([ID::new(1, 11), ID::new(0, 6)])
    );
    assert_eq!(doc.state_vv(), vv!(1 => 5, 0 => 7));
    assert_eq!(doc.oplog_vv(), vv!(1 => 12, 0 => 7));

    doc.checkout_to_latest();
    assert_ne!(
        0,
        doc.peer_id(),
        "peer id should be changed after checkout in detached editing mode"
    );
    assert_eq!(doc.get_text("text").to_string(), "Hello alice! world!");
    assert_eq!(&**string.lock().unwrap(), "Hello alice! world!");
    assert_eq!(doc.state_frontiers(), doc.oplog_frontiers());
    assert_eq!(doc.oplog_vv(), doc.state_vv());
}

#[test]
fn undo_still_works_after_detached_editing() {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    doc.set_peer_id(1).unwrap();
    doc.get_text("text").insert(0, "Hello").unwrap();
    doc.commit();
    doc.get_text("text").insert(5, " world!").unwrap();
    doc.commit();
    undo.undo().unwrap();
    assert_eq!(doc.get_text("text").to_string(), "Hello");
    undo.redo().unwrap();
    assert_eq!(doc.get_text("text").to_string(), "Hello world!");

    doc.set_detached_editing(true);
    doc.checkout(&Frontiers::from(ID::new(1, 4))).unwrap();
    assert!(!undo.can_undo());
    assert!(!undo.can_redo());
    doc.get_text("text").insert(5, " alice!").unwrap();
    doc.commit();
    assert!(undo.can_undo());
    assert!(!undo.can_redo());
    undo.undo().unwrap();
    assert_eq!(doc.get_text("text").to_string(), "Hello");
    assert!(!undo.can_undo());
    assert!(undo.can_redo());
    undo.redo().unwrap();
    assert_eq!(doc.get_text("text").to_string(), "Hello alice!");
}

/// A doc with `7@0..=4`, detached at `7@2` with detached editing on.
fn detached_before_the_ops_of_peer_7() -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(7).unwrap();
    doc.get_text("t").insert(0, "abc").unwrap();
    doc.commit();
    doc.get_text("t").insert(3, "de").unwrap();
    doc.commit();
    doc.set_detached_editing(true);
    doc.checkout(&Frontiers::from(ID::new(7, 2))).unwrap();
    doc
}

fn assert_peer_conflict(err: LoroError, peer: u64) {
    match err {
        LoroError::ConcurrentOpsWithSamePeerID { peer: p, .. } => assert_eq!(p, peer),
        other => panic!("expected ConcurrentOpsWithSamePeerID, got {other:?}"),
    }
}

/// Renewing the transaction panicked under the txn lock: the doc stayed poisoned,
/// and dropping it aborted the process.
#[test]
fn writing_with_a_peer_that_has_later_ops_returns_the_error() {
    let doc = detached_before_the_ops_of_peer_7();
    doc.set_peer_id(7).unwrap();

    let text = doc.get_text("t");
    assert_peer_conflict(text.insert(1, "XY").unwrap_err(), 7);
    assert_peer_conflict(doc.get_map("m").insert("k", 1).unwrap_err(), 7);
    match text.update("aXbc", Default::default()) {
        Err(UpdateTimeoutError::Loro(err)) => assert_peer_conflict(err, 7),
        other => panic!("unexpected result {other:?}"),
    }

    // Nothing was written, and the doc still reads, commits and exports.
    doc.commit();
    assert_eq!(text.to_string(), "abc");
    assert_eq!(doc.get_pending_txn_len(), 0);
    assert_eq!(doc.oplog_vv().get(&7), Some(&5));
    doc.export(ExportMode::all_updates()).unwrap();
    assert_eq!(doc.fork().get_text("t").to_string(), "abc");

    // Another peer can write from here.
    doc.set_peer_id(8).unwrap();
    text.insert(1, "XY").unwrap();
    doc.commit();
    assert_eq!(text.to_string(), "aXYbc");
}

#[test]
fn checkout_recovers_from_a_peer_that_cannot_write() {
    let doc = detached_before_the_ops_of_peer_7();
    doc.set_peer_id(7).unwrap();
    assert_peer_conflict(doc.get_text("t").insert(0, "x").unwrap_err(), 7);

    doc.checkout_to_latest();
    assert_ne!(doc.peer_id(), 7);
    doc.get_text("t").insert(0, "x").unwrap();
    doc.commit();
    assert_eq!(doc.get_text("t").to_string(), "xabcde");
}

#[test]
fn doc_level_writes_with_a_peer_that_cannot_write_return_the_error() {
    let doc = detached_before_the_ops_of_peer_7();
    doc.get_text("t").insert(0, "x").unwrap();
    doc.commit();
    let version = doc.state_frontiers();
    doc.set_peer_id(7).unwrap();

    let diff = doc.diff(&Frontiers::from(ID::new(7, 2)), &version).unwrap();
    assert_peer_conflict(doc.apply_diff(diff).unwrap_err(), 7);
    assert_peer_conflict(
        doc.revert_to(&Frontiers::from(ID::new(7, 2))).unwrap_err(),
        7,
    );
    assert_eq!(doc.get_text("t").to_string(), "xabc");
}

/// An `UndoManager` follows the doc's peer, so only ops imported for that peer can
/// make it unable to write. A failed undo keeps its item.
#[test]
fn undo_with_a_peer_that_cannot_write_returns_the_error() {
    let doc = detached_before_the_ops_of_peer_7();
    doc.set_peer_id(9).unwrap();
    let mut undo = UndoManager::new(&doc);
    doc.get_text("t").insert(0, "x").unwrap();
    doc.commit();

    let other = doc.fork();
    other.set_peer_id(9).unwrap();
    other.get_map("m").insert("k", 1).unwrap();
    other.commit();
    doc.import(&other.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    assert_peer_conflict(undo.undo().unwrap_err(), 9);
    assert_eq!(doc.get_text("t").to_string(), "xabc");
    assert!(undo.can_undo());
}

/// `set_peer_id` accepted the peer. The import is what makes it unusable, because
/// a detached doc takes in the peer's ops without moving its state.
#[test]
fn import_that_makes_the_peer_unusable_keeps_the_doc_working() {
    let doc = detached_before_the_ops_of_peer_7();
    doc.set_peer_id(9).unwrap();

    let remote = LoroDoc::new();
    remote.set_peer_id(9).unwrap();
    remote.get_map("remote").insert("k", 1).unwrap();
    remote.commit();
    doc.import(&remote.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_peer_conflict(doc.get_text("t").insert(0, "z").unwrap_err(), 9);

    doc.checkout_to_latest();
    doc.get_text("t").insert(0, "z").unwrap();
    doc.commit();
    assert_eq!(doc.get_text("t").to_string(), "zabcde");
}
