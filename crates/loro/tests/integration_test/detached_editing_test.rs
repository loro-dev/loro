use std::sync::{Arc, Mutex};

use super::gen_action;
use loro::{
    json::JsonChange, undo::UndoItemMeta, Frontiers, JsonSchema, LoroDoc, LoroError, UndoManager,
    ID,
};
use loro_internal::vv;
use tracing::{trace, trace_span};

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
    doc.subscribe_root(Arc::new(move |batch| {
        for e in batch.events {
            match e.diff {
                loro::event::Diff::Text(vec) => {
                    let mut s = string_clone.try_lock().unwrap();
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
    assert_eq!(&**string.try_lock().unwrap(), "Hello alice!");
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
    assert_eq!(&**string.try_lock().unwrap(), "Hello world! alice!");
    assert_eq!(doc.state_frontiers(), doc.oplog_frontiers());
    assert_eq!(doc.oplog_vv(), doc.state_vv());

    // New op on peer id 1
    doc.set_peer_id(1).unwrap();
    doc.get_text("text").insert(0, "Hi ").unwrap();
    doc.commit();
    assert_eq!(&**string.try_lock().unwrap(), "Hi Hello world! alice!");
    assert_eq!(doc.get_text("text").to_string(), "Hi Hello world! alice!");
    assert_eq!(doc.state_frontiers(), doc.oplog_frontiers());
    assert_eq!(doc.oplog_vv(), doc.state_vv());
    assert_eq!(doc.state_frontiers(), Frontiers::from_id(ID::new(1, 14)));
    let json = doc.export_json_updates(&Default::default(), &doc.oplog_vv());
    let actual = format!("{:#?}", json);
    let expected = r#"JsonSchema {
    schema_version: 1,
    start_version: Frontiers(
        [],
    ),
    peers: [
        1,
        2,
    ],
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
                6@1,
                11@0,
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
    doc.subscribe_root(Arc::new(move |batch| {
        for e in batch.events {
            match e.diff {
                loro::event::Diff::Text(vec) => {
                    let mut s = string_clone.try_lock().unwrap();
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
    assert_eq!(&**string.try_lock().unwrap(), "Hello alice!");
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
    assert_eq!(&**string.try_lock().unwrap(), "Hello alice! world!");
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
    undo.undo(&doc).unwrap();
    assert_eq!(doc.get_text("text").to_string(), "Hello");
    undo.redo(&doc).unwrap();
    assert_eq!(doc.get_text("text").to_string(), "Hello world!");

    doc.set_detached_editing(true);
    doc.checkout(&Frontiers::from(ID::new(1, 4))).unwrap();
    assert!(!undo.can_undo());
    assert!(!undo.can_redo());
    doc.get_text("text").insert(5, " alice!").unwrap();
    doc.commit();
    assert!(undo.can_undo());
    assert!(!undo.can_redo());
    undo.undo(&doc).unwrap();
    assert_eq!(doc.get_text("text").to_string(), "Hello");
    assert!(!undo.can_undo());
    assert!(undo.can_redo());
    undo.redo(&doc).unwrap();
    assert_eq!(doc.get_text("text").to_string(), "Hello alice!");
}
