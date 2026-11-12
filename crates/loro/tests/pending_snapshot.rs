use loro::{ContainerTrait, ExportMode, LoroDoc, StyleConfig, StyleConfigMap, VersionVector};
use std::sync::{Arc, Mutex};

// #1215: snapshots must apply parked changes to state, including chained deletes,
// before another local edit can record positions against that state.
#[test]
fn pending_snapshot_richtext_roundtrip() {
    for retained in [1, 300] {
        for shallow in [false, true] {
            for batch in [false, true] {
                for chained in [false, true] {
                    let source = LoroDoc::new();
                    source.set_peer_id(1).unwrap();
                    let mut styles = StyleConfigMap::new();
                    styles.insert(
                        "prov".into(),
                        StyleConfig::new().expand(loro::ExpandType::None),
                    );
                    source.config_text_style(styles);
                    let text = source.get_text("text");
                    text.insert(0, "hello world, this is a test").unwrap();
                    text.mark(0..5, "prov", "h").unwrap();
                    source.commit();
                    // Keep a snapshot-only empty root, and an untouched lazy container.
                    source.get_map("empty");
                    source.get_text("untouched").insert(0, "lazy").unwrap();
                    source.commit();
                    let root = source.state_frontiers();
                    text.insert(text.len_unicode(), &"!".repeat(retained))
                        .unwrap();
                    source.commit();
                    let snapshot = source
                        .export(if shallow {
                            ExportMode::shallow_snapshot(&root)
                        } else {
                            ExportMode::Snapshot
                        })
                        .unwrap();
                    let base = source.oplog_vv();
                    text.delete(0, 6).unwrap();
                    source.commit();
                    let u1 = source.export(ExportMode::updates(&base)).unwrap();
                    let v1 = source.oplog_vv();
                    text.delete(5, 3).unwrap();
                    source.commit();
                    let u2 = source.export(ExportMode::updates(&v1)).unwrap();

                    let target = LoroDoc::new();
                    target.import(&u1).unwrap();
                    if chained {
                        target.import(&u2).unwrap();
                    }
                    let events = Arc::new(Mutex::new(Vec::new()));
                    let captured = events.clone();
                    let _sub = target.subscribe_root(Arc::new(move |e| {
                        for d in e.events {
                            if let loro::event::Diff::Text(delta) = &d.diff {
                                captured
                                    .lock()
                                    .unwrap()
                                    .push((d.target.clone(), delta.clone()));
                            }
                        }
                    }));

                    let status = if batch {
                        let empty = LoroDoc::new().export(ExportMode::all_updates()).unwrap();
                        target.import_batch(&[snapshot, empty]).unwrap()
                    } else {
                        target.import(&snapshot).unwrap()
                    };
                    assert!(status.pending.is_none(), "shallow={shallow}, batch={batch}");
                    if !chained {
                        target.import(&u2).unwrap();
                    }
                    assert_eq!(target.get_deep_value(), source.get_deep_value());
                    assert_eq!(target.state_frontiers(), target.oplog_frontiers());
                    assert_eq!(target.oplog_vv(), source.oplog_vv());
                    assert_eq!(target.get_text("text").to_delta(), text.to_delta());
                    let events = events.lock().unwrap();
                    assert!(!events.is_empty());
                    assert!(events
                        .iter()
                        .any(|(id, _)| id == &target.get_text("text").id()));
                    let mirror = LoroDoc::new();
                    mirror.config_text_style({
                        let mut styles = StyleConfigMap::new();
                        styles.insert(
                            "prov".into(),
                            StyleConfig::new().expand(loro::ExpandType::None),
                        );
                        styles
                    });
                    for (id, delta) in events.iter() {
                        if id == &target.get_text("text").id() {
                            mirror.get_text("text").apply_delta(delta).unwrap();
                        }
                    }
                    assert_eq!(
                        mirror.get_text("text").to_delta(),
                        target.get_text("text").to_delta()
                    );
                    drop(events);

                    target.get_text("text").insert(2, "🦑e\u{301}").unwrap();
                    target.get_text("text").delete(0, 3).unwrap();
                    target.commit();
                    let replay = LoroDoc::new();
                    replay
                        .import(
                            &source
                                .export(if shallow {
                                    ExportMode::shallow_snapshot(&root)
                                } else {
                                    ExportMode::all_updates()
                                })
                                .unwrap(),
                        )
                        .unwrap();
                    replay
                        .import(
                            &target
                                .export(ExportMode::updates(&source.oplog_vv()))
                                .unwrap(),
                        )
                        .unwrap();
                    assert_eq!(
                        replay.get_text("text").to_delta(),
                        target.get_text("text").to_delta()
                    );
                    let copy =
                        LoroDoc::from_snapshot(&target.export(ExportMode::Snapshot).unwrap())
                            .unwrap();
                    assert_eq!(copy.get_deep_value(), target.get_deep_value());
                    if !shallow {
                        let fresh = LoroDoc::new();
                        fresh
                            .import(
                                &target
                                    .export(ExportMode::updates(&VersionVector::default()))
                                    .unwrap(),
                            )
                            .unwrap();
                        assert_eq!(
                            fresh.get_text("text").to_delta(),
                            target.get_text("text").to_delta()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn issue_1215_unicode_styled_history() {
    let source = LoroDoc::new();
    source.set_peer_id(14413189213326905750).unwrap();
    let mut styles = StyleConfigMap::new();
    styles.insert(
        "prov".into(),
        StyleConfig::new().expand(loro::ExpandType::None),
    );
    source.config_text_style(styles);
    let text = source.get_text("text");
    text.insert_utf16(0, "\u{f0088}Z\u{e9}\u{4e2d}\u{f0089}")
        .unwrap();
    text.mark_utf16(0..7, "prov", "h").unwrap();
    let other = LoroDoc::new();
    other.set_peer_id(355662022648776645).unwrap();
    other.get_text("text").insert_utf16(0, "\u{f0092}\u{1f469}\u{200d}\u{1f469}\u{200d}\u{1f467}\n\n\u{4e2d}x\u{f0093}\u{f0094}e\u{301}\n \u{4e2d}\u{4e2d}\u{f0095}").unwrap();
    source
        .import(&other.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    text.insert_utf16(
        26,
        "\u{f00b0}\u{1f991}\u{f00b1}\u{f00b2}xe\u{301}b\u{f00b3}",
    )
    .unwrap();
    let snapshot = source.export(ExportMode::Snapshot).unwrap();
    let v0 = source.oplog_vv();
    text.delete_utf16(0, 16).unwrap();
    let u1 = source.export(ExportMode::updates(&v0)).unwrap();
    let v1 = source.oplog_vv();
    text.delete_utf16(16, 8).unwrap();
    let u2 = source.export(ExportMode::updates(&v1)).unwrap();
    let target = LoroDoc::new();
    target.import(&u1).unwrap();
    target.import(&snapshot).unwrap();
    target.import(&u2).unwrap();
    assert_eq!(target.get_text("text").to_delta(), text.to_delta());
    assert_eq!(target.state_frontiers(), source.state_frontiers());
    // The stale-state reproduction deletes at 40. Correct state is shorter;
    // that invalid request must return Err, leaving the document usable.
    assert!(target.get_text("text").delete_utf16(40, 7).is_err());
    target.get_text("text").delete_utf16(0, 7).unwrap();
    let history = target.export(ExportMode::all_updates()).unwrap();
    let replay = LoroDoc::new();
    replay.import(&history).unwrap();
    assert_eq!(
        replay.get_text("text").to_delta(),
        target.get_text("text").to_delta()
    );
    let empty = LoroDoc::new();
    empty.get_text("text");
    let copy = LoroDoc::from_snapshot(&empty.export(ExportMode::Snapshot).unwrap()).unwrap();
    copy.import(&target.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_eq!(
        copy.get_text("text").to_delta(),
        target.get_text("text").to_delta()
    );
}

#[test]
fn pending_overlapping_suffix_claim_with_cross_peer_dep_replays_cleanly() {
    let p1 = LoroDoc::new();
    p1.set_peer_id(1).unwrap();
    p1.get_text("text").insert(0, "base-").unwrap();
    p1.commit();

    let p2 = LoroDoc::new();
    p2.set_peer_id(2).unwrap();
    p2.import(&p1.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let after_p1 = p2.oplog_vv();
    p2.get_text("text").insert(5, "ab").unwrap();
    p2.commit();
    let u_prefix = p2.export(ExportMode::updates(&after_p1)).unwrap();
    p2.get_text("text").insert(7, "c").unwrap();
    p2.commit();
    let u_merged = p2.export(ExportMode::updates(&after_p1)).unwrap();
    let snap_source = p2.fork();
    let snapshot = snap_source.export(ExportMode::Snapshot).unwrap();

    let after_merged = p2.oplog_vv();
    p2.get_text("text").insert(8, "!").unwrap();
    p2.commit();
    let u_tail = p2.export(ExportMode::updates(&after_merged)).unwrap();

    let target = LoroDoc::new();
    assert!(target.import(&u_prefix).unwrap().pending.is_some());
    assert!(target.import(&u_merged).unwrap().pending.is_some());
    assert!(target.import(&u_tail).unwrap().pending.is_some());

    let status = target.import(&snapshot).unwrap();
    assert!(status.pending.is_none());
    assert_eq!(target.get_text("text").to_string(), "base-abc!");
    assert_eq!(target.oplog_vv(), p2.oplog_vv());
    assert_eq!(target.state_frontiers(), target.oplog_frontiers());
}
