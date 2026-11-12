use base64::{engine::general_purpose::STANDARD, Engine};
use loro::{CommitOptions, ExportMode, JsonSchema, LoroDoc, LoroError, VersionVector, ID};

fn history() -> JsonSchema {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    a.get_map("m").insert("a", 1).unwrap();
    a.commit();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    b.get_map("m").insert("b", 2).unwrap();
    b.commit();
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    a.get_map("m").insert("c", 3).unwrap();
    a.commit_with(CommitOptions::new().commit_msg("third"));
    a.export_json_updates_without_peer_compression(&VersionVector::default(), &a.oplog_vv())
        .unwrap()
}

fn assert_usable(doc: &LoroDoc) {
    doc.try_checkout_to_latest().unwrap();
    doc.set_peer_id(99).unwrap();
    doc.get_map("m").insert("after", true).unwrap();
    doc.commit();
    for mode in [ExportMode::all_updates(), ExportMode::Snapshot] {
        let copy = LoroDoc::new();
        copy.import(&doc.export(mode).unwrap()).unwrap();
        assert_eq!(copy.get_deep_value(), doc.get_deep_value());
        let relay = LoroDoc::new();
        relay
            .import(&copy.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        assert_eq!(relay.get_deep_value(), doc.get_deep_value());
    }
}

#[test]
fn issue_1239_binary_is_rejected_without_poisoning() {
    let bytes = STANDARD.decode("bG9ybwAAAAAAAAAAAAAAAI/2aRIABDoAAQABARABAQAAAAAAAAABAQAAAAAABQEAAAEABgEEAQIAAAIBdAAOAQQCAQACAQACAQUCAQEAAgFhVwACAQICHgICAAAAAAAAAAEAAAAAAAAAAQIDAQABAQEAAAECABIBAAEAAwUGZmlyc3RzZWNvbmQGAQQBAgAAAgF0AA4BBAIEAAIEAgIEBQIEAQAEAWIBYw==").unwrap();
    for detached in [false, true] {
        for batch in [false, true] {
            let doc = LoroDoc::new();
            if detached {
                doc.detach();
            }
            let result = if batch {
                doc.import_batch(&[bytes.clone(), bytes.clone()])
            } else {
                doc.import(&bytes)
            };
            assert!(
                matches!(result, Err(LoroError::DecodeError(_))),
                "{result:?}"
            );
            assert!(doc.oplog_vv().is_empty());
            assert_usable(&doc);
        }
    }
}

#[test]
fn missing_or_unrelated_deps_do_not_authorize_existing_predecessor() {
    for deps in [vec![], vec![ID::new(2, 0)]] {
        // Peer 2's change is concurrent with the forged second change of peer 1:
        // remove its dependency on peer 1 too.
        let mut json = history();
        json.changes
            .iter_mut()
            .find(|c| c.id == ID::new(2, 0))
            .unwrap()
            .deps
            .clear();
        json.changes
            .iter_mut()
            .find(|c| c.id == ID::new(1, 1))
            .unwrap()
            .deps = deps;
        for detached in [false, true] {
            for prefix in [0, 1, 2] {
                let doc = LoroDoc::new();
                if detached {
                    doc.detach();
                }
                if prefix > 0 {
                    let mut first = json.clone();
                    first.changes.retain(|c| {
                        c.id == ID::new(1, 0) || (prefix == 2 && c.id == ID::new(2, 0))
                    });
                    doc.import_json_updates(first).unwrap();
                }
                let before = doc.oplog_vv();
                let before_value = doc.get_deep_value();
                let err = doc.import_json_updates(json.clone()).unwrap_err();
                assert!(matches!(err, LoroError::DecodeError(_)), "{err:?}");
                assert_eq!(doc.oplog_vv(), before);
                assert_eq!(doc.get_deep_value(), before_value);
                assert_usable(&doc);
            }
        }
    }
}

#[test]
fn indirect_predecessor_is_valid_in_same_blob_local_history_and_shallow_history() {
    let json = history();
    let last = json.changes.iter().find(|c| c.id == ID::new(1, 1)).unwrap();
    assert_eq!(last.deps, vec![ID::new(2, 0)]);
    for prefix in [false, true] {
        for shallow in [false, true] {
            let doc = LoroDoc::new();
            if prefix || shallow {
                let source = LoroDoc::new();
                let mut first = json.clone();
                first.changes.retain(|c| c.id != ID::new(1, 1));
                source.import_json_updates(first).unwrap();
                let frontiers = source.oplog_frontiers();
                let mode = if shallow {
                    ExportMode::shallow_snapshot(&frontiers)
                } else {
                    ExportMode::Snapshot
                };
                doc.import(&source.export(mode).unwrap()).unwrap();
            }
            doc.import_json_updates(json.clone()).unwrap();
            assert_eq!(doc.get_map("m").len(), 3);
            // Full updates cannot bootstrap a shallow doc; its snapshot can.
            let copy = LoroDoc::new();
            copy.import(&doc.export(ExportMode::Snapshot).unwrap())
                .unwrap();
            assert_eq!(copy.get_deep_value(), doc.get_deep_value());
            if !shallow {
                assert_usable(&doc);
            }
        }
    }
}

#[test]
fn pending_indirect_dependency_is_checked_before_unlocking() {
    for valid in [false, true] {
        for multi_hop in [false, true] {
            for prefix_first in [false, true] {
                let mut json = history();
                if !valid {
                    json.changes
                        .iter_mut()
                        .find(|c| c.id == ID::new(2, 0))
                        .unwrap()
                        .deps
                        .clear();
                }
                if multi_hop {
                    let mut bridge = json
                        .changes
                        .iter()
                        .find(|c| c.id == ID::new(2, 0))
                        .unwrap()
                        .clone();
                    bridge.id.peer = 3;
                    bridge.deps = vec![ID::new(2, 0)];
                    json.changes.push(bridge);
                    json.changes
                        .iter_mut()
                        .find(|c| c.id == ID::new(1, 1))
                        .unwrap()
                        .deps = vec![ID::new(3, 0)];
                }
                let mut pending = json.clone();
                pending.changes.retain(|c| c.id == ID::new(1, 1));
                let mut prefix = json.clone();
                prefix.changes.retain(|c| c.id == ID::new(1, 0));
                let mut dep = json.clone();
                dep.changes.retain(|c| c.id == ID::new(2, 0));
                let doc = LoroDoc::new();
                if multi_hop {
                    let mut bridge = json;
                    bridge.changes.retain(|c| c.id == ID::new(3, 0));
                    doc.import_json_updates(bridge).unwrap();
                }
                if prefix_first {
                    doc.import_json_updates(prefix).unwrap();
                    doc.import_json_updates(pending).unwrap();
                } else {
                    doc.import_json_updates(pending).unwrap();
                    doc.import_json_updates(prefix).unwrap();
                }
                let before = doc.oplog_vv();
                let result = doc.import_json_updates(dep);
                if valid {
                    result.unwrap();
                    assert_eq!(doc.get_map("m").len(), 3);
                    assert_usable(&doc);
                } else {
                    assert!(
                        matches!(result, Err(LoroError::DecodeError(_))),
                        "{result:?}"
                    );
                    assert_eq!(doc.oplog_vv(), before);
                    assert_usable(&doc);
                }
            }
        }
    }
}

#[test]
fn interleaved_multi_peer_batch_and_pending_imports_stay_fast_and_reject_cycles() {
    let source = LoroDoc::new();
    for step in 0..1500u64 {
        let peer = 1 + (step % 150);
        source.set_peer_id(peer).unwrap();
        source
            .get_map("m")
            .insert(&format!("k{}", peer), step as i64)
            .unwrap();
        source.commit();
        if step == 149 {
            // Capture base after each of the 150 peers has written counter 0.
        }
    }
    let full = source.export(ExportMode::all_updates()).unwrap();
    let receiver = LoroDoc::new();
    receiver.import(&full).unwrap();
    assert_eq!(receiver.get_deep_value(), source.get_deep_value());

    // Mutual cross-peer dependency cycle (`1@1 <-> 1@2`) cannot causally cover `0@1` or `0@2`.
    let mut json = history();
    json.changes
        .retain(|c| c.id == ID::new(1, 0) || c.id == ID::new(1, 1));
    let mut peer2 = json
        .changes
        .iter()
        .find(|c| c.id == ID::new(1, 1))
        .unwrap()
        .clone();
    peer2.id = ID::new(2, 1);
    peer2.deps = vec![ID::new(1, 1)];
    json.changes
        .iter_mut()
        .find(|c| c.id == ID::new(1, 1))
        .unwrap()
        .deps = vec![ID::new(2, 1)];
    json.changes.push(peer2);

    let doc = LoroDoc::new();
    let err = doc.import_json_updates(json).unwrap_err();
    assert!(matches!(err, LoroError::DecodeError(_)), "{err:?}");
    assert!(doc.oplog_vv().is_empty());
    assert_usable(&doc);
}
