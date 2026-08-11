use std::{
    borrow::Cow,
    sync::{atomic::AtomicBool, Arc},
};

use super::gen_action;
use loro::{
    cursor::CannotFindRelativePosition, ExpandType, ExportMode, Frontiers, LoroDoc, LoroValue,
    StyleConfig, StyleConfigMap, ID,
};

/// Byte-level scan of an exported blob. Only used for *absence* checks, and
/// even those are best-effort: state KV blocks may be LZ4-compressed, which can
/// hide a retained secret from this scan. The authoritative leak assertions
/// decode the exported state structurally — see the tests in
/// `loro-internal/src/encoding/shallow_snapshot.rs`.
fn bytes_contain(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[test]
fn state_only_at_concurrent_frontiers_excludes_later_ops() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(0)?;
    doc.set_detached_editing(true);

    doc.get_list("list").insert(0, "Counter")?;
    let list_frontiers = doc.oplog_frontiers();

    doc.checkout(&Frontiers::default())?;
    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let root = tree.create(None)?;
    let mut target_frontiers = list_frontiers;
    target_frontiers.merge_with_greater(&doc.state_frontiers());
    let target_frontiers = doc
        .minimize_frontiers(&target_frontiers)
        .expect("target frontiers should be reachable");

    doc.checkout(&target_frontiers)?;
    let expected = doc.get_deep_value();

    doc.get_tree("tree").create(Some(root))?;
    let latest = doc.get_deep_value();
    assert_ne!(expected, latest);

    let bytes = doc.export(ExportMode::state_only(Some(&target_frontiers)))?;
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes)?;

    assert_eq!(new_doc.get_deep_value(), expected);
    Ok(())
}

#[test]
fn test_gc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 32);
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    gen_action(&doc, 123, 10);
    doc.commit();
    let shallow_bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers));

    let new_doc = LoroDoc::new();
    new_doc.import(&shallow_bytes.unwrap())?;
    assert_eq!(doc.get_deep_value(), new_doc.get_deep_value());
    Ok(())
}

#[test]
fn test_shallow_1() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "1")?;
    doc.get_text("text").insert(0, "2")?;
    doc.get_text("text").insert(0, "3")?;
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    doc.get_text("text").insert(3, "4")?;
    doc.commit();
    let shallow_bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers));

    let new_doc = LoroDoc::new();
    new_doc.import(&shallow_bytes.unwrap())?;
    assert_eq!(doc.get_deep_value(), new_doc.get_deep_value());
    Ok(())
}

#[test]
fn test_checkout_to_text_that_were_created_before_gc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "0")?;
    doc.get_text("text").insert(0, "1")?;
    doc.get_text("text").insert(0, "2")?;
    doc.get_text("text").insert(1, "3")?;
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    doc.get_text("text").delete(0, 3)?;
    let bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap())?;
    new_doc.checkout(&frontiers)?;
    assert_eq!(new_doc.get_text("text").to_string(), *"2310");
    Ok(())
}

#[test]
fn test_checkout_to_list_that_were_created_before_gc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_list("list").insert(0, 0)?;
    doc.get_list("list").insert(1, 1)?;
    doc.get_list("list").insert(2, 2)?;
    doc.get_list("list").insert(1, 3)?;
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    doc.get_list("list").delete(0, 3)?;
    let bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap())?;
    new_doc.checkout(&frontiers)?;
    assert_eq!(
        new_doc.get_list("list").to_vec(),
        vec![0.into(), 3.into(), 1.into(), 2.into()]
    );
    Ok(())
}

#[test]
fn test_checkout_to_movable_list_that_were_created_before_gc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_movable_list("list").insert(0, 0)?;
    doc.get_movable_list("list").insert(1, 1)?;
    doc.get_movable_list("list").insert(2, 2)?;
    doc.get_movable_list("list").insert(1, 3)?;
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    doc.get_movable_list("list").delete(0, 3)?;
    let bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap())?;
    new_doc.checkout(&frontiers)?;
    assert_eq!(
        new_doc.get_movable_list("list").to_vec(),
        vec![0.into(), 3.into(), 1.into(), 2.into()]
    );
    Ok(())
}

#[test]
fn shallow_on_the_given_version_when_feasible() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 64);
    doc.commit();
    let bytes = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(1, 31)));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap())?;
    assert_eq!(new_doc.shallow_since_vv().get(&1).copied().unwrap(), 31);
    Ok(())
}

#[test]
fn export_snapshot_on_a_shallow_doc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 32);
    doc.commit();

    // Get the current frontiers
    let frontiers = doc.oplog_frontiers();
    let old_value = doc.get_deep_value();
    gen_action(&doc, 123, 32);
    doc.commit();

    // Export using shallowSnapshot mode
    let bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers));

    // Import into a new document
    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&bytes.unwrap())?;
    assert_eq!(shallow_doc.shallow_since_vv().get(&1).copied().unwrap(), 31);
    let new_snapshot = shallow_doc.export(loro::ExportMode::Snapshot);

    let new_doc = LoroDoc::new();
    new_doc.import(&new_snapshot.unwrap())?;
    assert_eq!(new_doc.shallow_since_vv().get(&1).copied().unwrap(), 31);
    assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());
    new_doc.checkout(&frontiers)?;
    assert_eq!(new_doc.get_deep_value(), old_value);
    new_doc.checkout_to_latest();
    assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());
    Ok(())
}

#[test]
fn export_snapshot_on_shallow_doc_with_small_tail_updates() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 32);
    doc.commit();

    let shallow_frontiers = doc.oplog_frontiers();
    let shallow_value = doc.get_deep_value();
    gen_action(&doc, 456, 4);
    doc.commit();
    let latest_value = doc.get_deep_value();

    let shallow_bytes = doc.export(loro::ExportMode::shallow_snapshot(&shallow_frontiers))?;
    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&shallow_bytes)?;

    let snapshot_from_shallow = shallow_doc.export(loro::ExportMode::Snapshot)?;
    let restored = LoroDoc::new();
    restored.import(&snapshot_from_shallow)?;

    assert!(restored.is_shallow());
    assert_eq!(restored.shallow_since_frontiers(), shallow_frontiers);
    assert_eq!(restored.get_deep_value(), latest_value);
    restored.checkout(&shallow_frontiers)?;
    assert_eq!(restored.get_deep_value(), shallow_value);
    Ok(())
}

#[test]
fn export_snapshot_on_shallow_doc_with_large_tail_updates() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let text = doc.get_text("text");
    text.insert(0, &"a".repeat(64))?;
    doc.commit();

    let shallow_frontiers = doc.oplog_frontiers();
    let shallow_value = doc.get_deep_value();
    let shallow_bytes = doc.export(ExportMode::shallow_snapshot(&shallow_frontiers))?;

    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&shallow_bytes)?;
    let text = shallow_doc.get_text("text");
    text.delete(0, text.len_unicode())?;
    text.insert(0, &format!("{}{}", "b".repeat(64), "c".repeat(64)))?;
    shallow_doc.commit();
    assert!(shallow_doc.len_ops() > 16);
    let latest_value = shallow_doc.get_deep_value();

    let snapshot_from_shallow = shallow_doc.export(ExportMode::Snapshot)?;
    let restored = LoroDoc::new();
    restored.import(&snapshot_from_shallow)?;

    assert!(restored.is_shallow());
    assert_eq!(restored.shallow_since_frontiers(), shallow_frontiers);
    assert_eq!(restored.get_deep_value(), latest_value);
    restored.checkout(&shallow_frontiers)?;
    assert_eq!(restored.get_deep_value(), shallow_value);
    restored.checkout_to_latest();
    assert_eq!(restored.get_deep_value(), latest_value);
    Ok(())
}

#[test]
fn test_richtext_gc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let text = doc.get_text("text");
    text.insert(0, "1")?; // 0
    text.insert(0, "2")?; // 1
    text.insert(0, "3")?; // 2
    text.mark(0..2, "bold", "value")?; // 3, 4
    doc.commit();
    text.insert(3, "456")?; // 5, 6, 7
    let bytes = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(1, 3)));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap())?;
    new_doc.checkout(&Frontiers::from(ID::new(1, 4)))?;
    assert_eq!(new_doc.get_text("text").to_string(), "321");
    new_doc.checkout_to_latest();
    assert_eq!(new_doc.get_text("text").to_string(), "321456");
    Ok(())
}

#[test]
fn import_updates_depend_on_shallow_history_should_raise_error() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 4);
    doc.commit();
    let doc2 = doc.fork();
    doc2.get_text("text").insert(0, "1")?;
    doc2.commit();
    gen_action(&doc, 123, 2);
    doc.commit();
    let shallow_snapshot = doc.export(loro::ExportMode::shallow_snapshot(&doc.oplog_frontiers()));
    doc.get_text("hello").insert(0, "world").unwrap();
    doc2.import(
        &doc.export(loro::ExportMode::Updates {
            from: Cow::Borrowed(&doc2.oplog_vv()),
        })
        .unwrap(),
    )
    .unwrap();

    let new_doc = LoroDoc::new();
    new_doc.import(&shallow_snapshot.unwrap()).unwrap();

    let ran = Arc::new(AtomicBool::new(false));
    let ran_clone = ran.clone();
    let _sub = new_doc.subscribe_root(Arc::new(move |e| {
        ran_clone.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(e.events.len() == 1);
        match e.events[0].diff {
            loro::event::Diff::Text(_) => {}
            _ => {
                unreachable!()
            }
        }
    }));
    let result = new_doc.import(
        &doc2
            .export(loro::ExportMode::updates_owned(new_doc.oplog_vv()))
            .unwrap(),
    );
    assert!(result.is_err());
    // But updates from doc should be fine ("hello": "world")
    assert_eq!(new_doc.get_text("hello").to_string(), *"world");
    assert!(ran.load(std::sync::atomic::Ordering::Relaxed));
    Ok(())
}

#[test]
fn the_vv_on_shallow_doc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    gen_action(&doc, 0, 10);
    doc.commit();
    let snapshot = doc.export(loro::ExportMode::shallow_snapshot(&doc.oplog_frontiers()));
    let new_doc = LoroDoc::new();
    new_doc.import(&snapshot.unwrap()).unwrap();
    assert!(!new_doc.shallow_since_vv().is_empty());
    assert_eq!(new_doc.oplog_vv(), new_doc.state_vv());
    assert_eq!(new_doc.oplog_vv(), doc.state_vv());
    assert_eq!(new_doc.oplog_frontiers(), doc.oplog_frontiers());
    assert_eq!(new_doc.oplog_frontiers(), new_doc.state_frontiers());
    assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());

    gen_action(&doc, 0, 10);
    doc.commit();
    let bytes = doc.export(ExportMode::all_updates());
    new_doc.import(&bytes.unwrap()).unwrap();
    assert_eq!(new_doc.oplog_vv(), new_doc.state_vv());
    assert_eq!(new_doc.oplog_vv(), doc.state_vv());
    assert_eq!(new_doc.oplog_frontiers(), doc.oplog_frontiers());
    assert_eq!(new_doc.oplog_frontiers(), new_doc.state_frontiers());
    assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());

    Ok(())
}

#[test]
fn no_event_when_exporting_shallow_snapshot() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 0, 10);
    doc.commit();
    let _id = doc.subscribe_root(Arc::new(|_diff| {
        panic!("should not emit event");
    }));
    let _snapshot = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(1, 3)));
    Ok(())
}

#[test]
fn test_cursor_that_cannot_be_found_when_exporting_shallow_snapshot() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "Hello world")?;
    let c = doc
        .get_text("text")
        .get_cursor(3, loro::cursor::Side::Left)
        .unwrap();
    doc.get_text("text").delete(0, 5)?;
    doc.commit();
    let snapshot = doc.export(loro::ExportMode::shallow_snapshot(&doc.oplog_frontiers()));
    let new_doc = LoroDoc::new();
    new_doc.import(&snapshot.unwrap())?;
    let result = new_doc.get_cursor_pos(&c);
    match result {
        Ok(v) => {
            dbg!(v);
            unreachable!()
        }
        Err(CannotFindRelativePosition::HistoryCleared) => {}
        Err(x) => {
            dbg!(x);
            unreachable!()
        }
    }
    Ok(())
}

#[test]
fn test_cursor_that_can_be_found_when_exporting_shallow_snapshot() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "Hello world")?;
    doc.commit();
    let c = doc
        .get_text("text")
        .get_cursor(3, loro::cursor::Side::Left)
        .unwrap();
    doc.get_text("text").delete(0, 5)?;
    doc.commit();
    let snapshot = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(1, 10)));
    let new_doc = LoroDoc::new();
    new_doc.import(&snapshot.unwrap())?;
    let result = new_doc.get_cursor_pos(&c);
    match result {
        Ok(v) => {
            assert_eq!(v.current.pos, 0);
        }
        Err(x) => {
            dbg!(x);
            unreachable!()
        }
    }
    Ok(())
}

#[test]
fn test_export_shallow_snapshot_from_shallow_doc() -> anyhow::Result<()> {
    // Create and populate the original document
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 32);
    doc.commit();

    // Get the current frontiers and create some more actions
    let frontiers = doc.oplog_frontiers();
    gen_action(&doc, 123, 32);
    doc.commit();

    // Export using shallowSnapshot mode
    let shallow_bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers))?;

    // Import into a new document
    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&shallow_bytes)?;

    // Attempt to export a shallow snapshot from the shallow document
    // using frontiers before its shallow version
    let result = shallow_doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(1, 16)));

    // The export should fail because the requested frontiers are before the shallow version
    assert!(result.is_err());

    if let Err(e) = result {
        assert!(matches!(e, loro::LoroEncodeError::FrontiersNotFound(..)));
    } else {
        panic!("Expected an error, but got Ok");
    }

    Ok(())
}

/// Regression for a branch-specific import bug on `feat/diff-text-lca-review`.
///
/// Setup: 3 peers each commit a few ops, then sync. Peer 1 then commits enough
/// post-mid_f ops that `shallow_snapshot(&mid_f)` actually trims peer 1's
/// pre-mid_f history (`shallow_since_vv = {1: 2}`,
/// `shallow_since_frontiers = [2@1]`). A second peer commits one cross-peer op
/// whose deps equal mid_f (`[2@1, 2@2, 2@3]`).
///
/// The import preflight should not reuse checkout's conservative
/// `is_before_shallow_root` semantics here: deps that touch the boundary
/// together with valid same-or-other-peer post-shallow ids should be
/// importable.
#[test]
fn shallow_doc_accepts_cross_peer_op_whose_deps_include_boundary() -> anyhow::Result<()> {
    let p1 = LoroDoc::new();
    p1.set_peer_id(1)?;
    let p2 = LoroDoc::new();
    p2.set_peer_id(2)?;
    let p3 = LoroDoc::new();
    p3.set_peer_id(3)?;

    for _ in 0..3 {
        p1.get_text("t").insert(0, "1")?;
        p1.commit();
        p2.get_text("t").insert(0, "2")?;
        p2.commit();
        p3.get_text("t").insert(0, "3")?;
        p3.commit();
    }
    let docs = [&p1, &p2, &p3];
    for i in 0..3 {
        for j in 0..3 {
            if i != j {
                docs[j].import(&docs[i].export(ExportMode::all_updates())?)?;
            }
        }
    }

    let mid_f = p1.oplog_frontiers();
    for k in 0..5 {
        p1.get_text("t").insert(0, &format!("{k}"))?;
        p1.commit();
    }

    let snap = p1.export(ExportMode::shallow_snapshot(&mid_f))?;
    let s = LoroDoc::new();
    s.import(&snap)?;
    assert!(
        !s.shallow_since_vv().is_empty(),
        "test setup must produce a real shallow trim"
    );

    p2.get_text("t").insert(0, "Y")?;
    p2.commit();

    let p2_updates = p2.export(ExportMode::all_updates())?;
    s.import(&p2_updates)
        .expect("shallow doc should accept cross-peer op whose deps include the shallow boundary");

    Ok(())
}

/// Shallow snapshots are documented as a content-redaction mechanism: exporting at the
/// current frontiers is supposed to drop the trimmed history, leaving only the live state.
/// The value of a rich-text style op whose whole range has been deleted must be dropped
/// just like deleted character content is.
///
/// Regression test contributed in <https://github.com/loro-dev/loro/pull/1057> by @nightscape.
#[test]
fn shallow_snapshot_drops_deleted_text_and_dead_style_values() -> anyhow::Result<()> {
    const SECRET_STYLE_VALUE: &str = "SECRET-STYLE-VALUE-e5f1";
    const SECRET_TEXT: &str = "SECRET-TEXT-a7b2";

    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let text = doc.get_text("text");
    text.insert(0, SECRET_TEXT)?;
    let len = SECRET_TEXT.chars().count();
    text.mark(
        0..len,
        "comment",
        LoroValue::String(SECRET_STYLE_VALUE.into()),
    )?;
    doc.commit();

    text.delete(0, len)?;
    doc.commit();

    // Both secrets are unreachable through every read API.
    assert_eq!(text.to_string(), "");
    let live_state = format!("{:?}", doc.get_deep_value());
    assert!(!live_state.contains(SECRET_TEXT));
    assert!(!live_state.contains(SECRET_STYLE_VALUE));

    let shallow = doc.export(ExportMode::shallow_snapshot(&doc.oplog_frontiers()))?;

    assert!(
        !bytes_contain(&shallow, SECRET_TEXT),
        "deleted text content must not survive a shallow snapshot"
    );
    assert!(
        !bytes_contain(&shallow, SECRET_STYLE_VALUE),
        "style value of a fully deleted range must not survive a shallow snapshot"
    );

    // Re-export from the imported shallow doc (the reuse-existing-root-bytes path)
    // must stay clean and importable.
    let imported = LoroDoc::new();
    imported.import(&shallow)?;
    let reexported = imported.export(ExportMode::shallow_snapshot(&imported.oplog_frontiers()))?;
    assert!(
        !bytes_contain(&reexported, SECRET_STYLE_VALUE),
        "style value must not survive re-export of an imported shallow snapshot"
    );
    let imported_again = LoroDoc::new();
    imported_again.import(&reexported)?;
    assert_eq!(imported_again.get_deep_value(), doc.get_deep_value());

    Ok(())
}

/// An empty both-expand style pair still captures future inserts, so its value is
/// live data: redacting it would diverge from full-history replicas. It must be
/// kept, and both replicas must agree after typing into the collapsed range.
#[test]
fn shallow_snapshot_keeps_live_both_expand_style_value() -> anyhow::Result<()> {
    fn cfg() -> StyleConfigMap {
        let mut map = StyleConfigMap::new();
        map.insert(
            "hl".into(),
            StyleConfig {
                expand: ExpandType::Both,
            },
        );
        map
    }

    let a = LoroDoc::new();
    a.set_peer_id(1)?;
    a.config_text_style(cfg());
    let ta = a.get_text("text");
    ta.insert(0, "abcd")?;
    ta.mark(1..3, "hl", LoroValue::String("BOTH-EXPAND-VALUE".into()))?;
    a.commit();
    ta.delete(1, 2)?;
    a.commit();

    let bytes = a.export(ExportMode::shallow_snapshot(&a.oplog_frontiers()))?;

    // The value's survival is asserted semantically below: after typing into
    // the collapsed range, the shallow replica must re-apply it.
    let b = LoroDoc::new();
    b.config_text_style(cfg());
    b.import(&bytes)?;

    // Typing into the collapsed range picks the style back up on both replicas.
    let vv = a.oplog_vv();
    ta.insert(1, "x")?;
    a.commit();
    b.import(&a.export(ExportMode::updates(&vv))?)?;

    let rendered_a = format!("{:?}", ta.get_richtext_value());
    let rendered_b = format!("{:?}", b.get_text("text").get_richtext_value());
    assert_eq!(rendered_a, rendered_b);
    assert!(rendered_b.contains("BOTH-EXPAND-VALUE"));
    Ok(())
}

/// A style that is alive at the requested shallow root and only dies in the
/// retained tail must keep its value: the imported doc can check out back to the
/// root and must render it. Re-exporting at the tip then drops it.
#[test]
fn shallow_snapshot_keeps_style_alive_at_root_and_redacts_on_reexport() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let text = doc.get_text("text");
    text.insert(0, "hello")?;
    text.mark(0..5, "comment", LoroValue::String("KEEP-ME-42".into()))?;
    doc.commit();
    let root_frontiers = doc.oplog_frontiers();
    text.delete(0, 5)?;
    doc.commit();

    // The value's retention is asserted semantically below: the imported doc
    // checks out back to the root and must render the style.
    let bytes = doc.export(ExportMode::shallow_snapshot(&root_frontiers))?;

    let imported = LoroDoc::new();
    imported.import(&bytes)?;
    assert_eq!(imported.get_text("text").to_string(), "");
    imported.checkout(&root_frontiers)?;
    let styled = format!("{:?}", imported.get_text("text").get_richtext_value());
    assert!(styled.contains("KEEP-ME-42"));
    imported.checkout_to_latest();

    // At the imported doc's tip the pair is dead, so a re-export rooted there
    // must drop the value.
    let reexported = imported.export(ExportMode::shallow_snapshot(&imported.oplog_frontiers()))?;
    assert!(!bytes_contain(&reexported, "KEEP-ME-42"));
    Ok(())
}

/// When the retained-op count exceeds the threshold the export also carries the
/// encoded latest state, which contains the same dead anchors and must be
/// redacted with the same pair set.
#[test]
fn shallow_snapshot_with_latest_state_bytes_redacts_dead_style_values() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let text = doc.get_text("text");
    text.insert(0, "secret")?;
    text.mark(
        0..6,
        "comment",
        LoroValue::String("SECRET-STYLE-9d3c".into()),
    )?;
    doc.commit();
    text.delete(0, 6)?;
    doc.commit();
    let start = doc.oplog_frontiers();

    // Make the latest text state differ from the root state so its entry
    // survives `remove_same` and must be redacted via the whitelist, and push
    // the retained-op count above the no-latest-state threshold.
    text.insert(0, "later")?;
    let list = doc.get_list("filler");
    for i in 0..300 {
        list.push(i)?;
    }
    doc.commit();

    let bytes = doc.export(ExportMode::shallow_snapshot(&start))?;
    assert!(!bytes_contain(&bytes, "SECRET-STYLE-9d3c"));

    let imported = LoroDoc::new();
    imported.import(&bytes)?;
    assert_eq!(imported.get_deep_value(), doc.get_deep_value());
    Ok(())
}

#[test]
fn state_only_snapshot_redacts_dead_style_values() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let text = doc.get_text("text");
    text.insert(0, "abc")?;
    text.mark(0..3, "comment", LoroValue::String("SECRET-SO-77".into()))?;
    doc.commit();
    text.delete(0, 3)?;
    doc.commit();

    let f = doc.oplog_frontiers();
    let bytes = doc.export(ExportMode::state_only(Some(&f)))?;
    assert!(!bytes_contain(&bytes, "SECRET-SO-77"));

    let imported = LoroDoc::new();
    imported.import(&bytes)?;
    assert_eq!(imported.get_text("text").to_string(), "");
    Ok(())
}

/// Dead pairs of every non-both expand kind are redacted, and typing at the
/// collapsed position behaves identically on the exporter and the shallow
/// replica afterwards.
#[test]
fn shallow_snapshot_redacts_dead_styles_for_all_non_both_expands() -> anyhow::Result<()> {
    for expand in [ExpandType::After, ExpandType::Before, ExpandType::None] {
        let cfg = || {
            let mut map = StyleConfigMap::new();
            map.insert("hl".into(), StyleConfig { expand });
            map
        };

        let a = LoroDoc::new();
        a.set_peer_id(1)?;
        a.config_text_style(cfg());
        let ta = a.get_text("text");
        ta.insert(0, "abcd")?;
        ta.mark(1..3, "hl", LoroValue::String("SECRET-EXP-11".into()))?;
        a.commit();
        ta.delete(1, 2)?;
        a.commit();

        let bytes = a.export(ExportMode::shallow_snapshot(&a.oplog_frontiers()))?;
        assert!(
            !bytes_contain(&bytes, "SECRET-EXP-11"),
            "expand={expand:?}: dead style value must be redacted"
        );

        let b = LoroDoc::new();
        b.config_text_style(cfg());
        b.import(&bytes)?;

        let vv = a.oplog_vv();
        ta.insert(1, "x")?;
        a.commit();
        b.import(&a.export(ExportMode::updates(&vv))?)?;
        assert_eq!(
            format!("{:?}", ta.get_richtext_value()),
            format!("{:?}", b.get_text("text").get_richtext_value()),
            "expand={expand:?}"
        );
    }
    Ok(())
}
