//! Importing history whose op ids are already used locally by *different* ops
//! (two clients sharing a peer id) must fail with an `Err` before anything is
//! applied, instead of panicking, poisoning the doc, or scrambling its state.
//! See loro-dev/loro#1118.

use loro::{
    ExportMode, LoroDoc, LoroError, LoroResult, LoroValue, ToJson, TreeParentId, VersionVector, ID,
};
use pretty_assertions::assert_eq;

struct Before {
    value: serde_json::Value,
    vv: VersionVector,
    frontiers: loro::Frontiers,
}

fn snapshot_of(doc: &LoroDoc) -> Before {
    Before {
        value: doc.get_deep_value().to_json_value(),
        vv: doc.oplog_vv(),
        frontiers: doc.oplog_frontiers(),
    }
}

fn assert_unchanged(doc: &LoroDoc, before: &Before) {
    assert_eq!(doc.get_deep_value().to_json_value(), before.value);
    assert_eq!(doc.oplog_vv(), before.vv);
    assert_eq!(doc.oplog_frontiers(), before.frontiers);
    assert_eq!(doc.state_frontiers(), before.frontiers);
}

fn assert_used_op_id(err: LoroError, expected: ID) {
    match err {
        LoroError::UsedOpID { id } => assert_eq!(id, expected),
        other => panic!("expected UsedOpID({expected}), got {other:?}"),
    }
}

/// After a rejected import the doc keeps working: it accepts local edits and
/// legitimate remote updates, and it can be exported and dropped.
fn assert_still_usable(doc: &LoroDoc) {
    doc.get_text("after").insert(0, "local").unwrap();
    doc.commit();

    let remote = LoroDoc::new();
    remote.set_peer_id(1000).unwrap();
    remote
        .import(&doc.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    remote.get_map("remote").insert("k", "v").unwrap();
    remote.commit();
    doc.import(&remote.export(ExportMode::updates(&doc.oplog_vv())).unwrap())
        .unwrap();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        remote.get_deep_value().to_json_value()
    );

    let copy = LoroDoc::new();
    copy.import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_eq!(
        copy.get_deep_value().to_json_value(),
        doc.get_deep_value().to_json_value()
    );
}

/// The reproduction from loro-dev/loro#1118: `history` and `other` both wrote
/// 7@0..=1, and `other` went on to 7@2. The import used to trim the known part of
/// `other`'s change and apply its tail (an insert at position 2) on top of
/// `history`'s text, which has length 1.
#[test]
fn import_overlapping_change_with_different_content_is_rejected() -> LoroResult<()> {
    let history = LoroDoc::new();
    history.set_peer_id(7)?;
    history.get_map("m").insert("k", 0)?;
    history.get_text("t").insert(0, "a")?;
    history.commit(); // 7@0, 7@1

    let other = LoroDoc::new();
    other.set_peer_id(7)?;
    other.get_text("t").insert(0, "xyz")?;
    other.commit(); // 7@0..=2

    let before = snapshot_of(&history);
    for update in [
        other.export(ExportMode::all_updates())?,
        other.export(ExportMode::Snapshot)?,
    ] {
        let err = history.import(&update).unwrap_err();
        assert_used_op_id(err, ID::new(7, 0));
        assert_unchanged(&history, &before);

        let err = history.import_batch(&[update]).unwrap_err();
        assert_used_op_id(err, ID::new(7, 0));
        assert_unchanged(&history, &before);
        assert!(!history.is_detached());
    }

    let json = other.export_json_updates(&Default::default(), &other.oplog_vv());
    let err = history.import_json_updates(json).unwrap_err();
    assert_used_op_id(err, ID::new(7, 0));
    assert_unchanged(&history, &before);

    assert_still_usable(&history);
    Ok(())
}

/// Same shape, but the shifted tail lands on a valid position, so the import used
/// to succeed and silently scramble the text.
#[test]
fn import_overlapping_change_with_same_shape_is_rejected() -> LoroResult<()> {
    let history = LoroDoc::new();
    history.set_peer_id(7)?;
    history.get_text("t").insert(0, "hello")?;
    history.commit(); // 7@0..=4

    let other = LoroDoc::new();
    other.set_peer_id(7)?;
    other.get_text("t").insert(0, "hellO world")?;
    other.commit(); // 7@0..=10

    let before = snapshot_of(&history);
    let err = history
        .import(&other.export(ExportMode::all_updates())?)
        .unwrap_err();
    assert_used_op_id(err, ID::new(7, 4));
    assert_unchanged(&history, &before);
    assert_still_usable(&history);
    Ok(())
}

/// The conflicting part is a separate change that is entirely known locally, and
/// the new part starts exactly at the local counter end, so there is no partially
/// overlapping change to trim.
#[test]
fn import_known_conflicting_change_followed_by_new_change_is_rejected() -> LoroResult<()> {
    let history = LoroDoc::new();
    history.set_peer_id(7)?;
    history.get_text("t").insert(0, "ab")?;
    history.commit(); // 7@0..=1

    let other = LoroDoc::new();
    other.set_peer_id(7)?;
    other.get_text("t").insert(0, "xy")?;
    other.commit(); // 7@0..=1
                    // Put a change from another peer in between so 7@2 starts a new change.
    let helper = LoroDoc::new();
    helper.set_peer_id(9)?;
    helper.import(&other.export(ExportMode::all_updates())?)?;
    helper.get_map("m").insert("k", 1)?;
    helper.commit();
    other.import(&helper.export(ExportMode::all_updates())?)?;
    other.get_text("t").insert(2, "z")?;
    other.commit(); // 7@2, deps [7@1, 9@0]

    let before = snapshot_of(&history);
    let err = history
        .import(&other.export(ExportMode::all_updates())?)
        .unwrap_err();
    assert_used_op_id(err, ID::new(7, 0));
    assert_unchanged(&history, &before);
    assert_still_usable(&history);
    Ok(())
}

/// A peer-id collision in history the import does not build on (nothing new is
/// imported) is not detected, and the import stays a no-op.
#[test]
fn import_with_only_known_conflicting_changes_is_a_no_op() -> LoroResult<()> {
    let history = LoroDoc::new();
    history.set_peer_id(7)?;
    history.get_text("t").insert(0, "ab")?;
    history.commit();

    let other = LoroDoc::new();
    other.set_peer_id(7)?;
    other.get_text("t").insert(0, "xy")?;
    other.commit();

    let before = snapshot_of(&history);
    let status = history.import(&other.export(ExportMode::all_updates())?)?;
    assert!(status.success.is_empty());
    assert!(status.pending.is_none());
    assert_unchanged(&history, &before);
    Ok(())
}

/// Builds a doc with every kind of op, syncing `replica` piecewise along the way so
/// its change store splits and merges the history differently from `source`'s.
fn build_source_and_piecewise_replica() -> LoroResult<(LoroDoc, LoroDoc)> {
    let source = LoroDoc::new();
    source.set_peer_id(1)?;
    let replica = LoroDoc::new();
    replica.set_peer_id(2)?;
    let sync = |from: &LoroDoc, to: &LoroDoc| -> LoroResult<()> {
        to.import(&from.export(ExportMode::updates(&to.oplog_vv()))?)?;
        Ok(())
    };

    let text = source.get_text("text");
    text.insert(0, "hello wörld 😀")?;
    source.commit();
    sync(&source, &replica)?;
    text.delete(3, 4)?; // forward
    for i in (0..3).rev() {
        text.delete(i, 1)?; // backward, single-atom deletes
    }
    text.mark(0..2, "bold", true)?;
    source.commit();
    sync(&source, &replica)?;

    let list = source.get_list("list");
    list.push(1)?;
    list.push("two")?;
    list.push(LoroValue::Double(f64::NAN))?;
    list.delete(0, 1)?;
    let map = source.get_map("map");
    map.insert("nan", f64::NAN)?;
    map.insert("bin", vec![1u8, 2, 3])?;
    map.delete("bin")?;
    source.commit();
    sync(&source, &replica)?;

    let mov = source.get_movable_list("mov");
    mov.push("a")?;
    mov.push("b")?;
    mov.push("c")?;
    mov.mov(0, 2)?;
    mov.set(1, "B")?;
    mov.delete(0, 1)?;
    let tree = source.get_tree("tree");
    let root = tree.create(TreeParentId::Root)?;
    let child = tree.create(root)?;
    tree.mov(child, TreeParentId::Root)?;
    tree.delete(root)?;
    let counter = source.get_counter("counter");
    counter.increment(1.5)?;
    counter.decrement(0.5)?;
    source.commit();

    // Changes from the replica's own peer in between, then more from `source`.
    sync(&source, &replica)?;
    replica.get_text("text").insert(0, ">")?;
    replica.commit();
    sync(&replica, &source)?;
    text.insert(1, "more")?;
    text.delete(0, 2)?;
    source.commit();
    Ok((source, replica))
}

/// Re-importing history the doc already has must still succeed, whatever the
/// export format and however the change store split or merged it.
#[test]
fn reimporting_known_history_with_new_changes_still_succeeds() -> LoroResult<()> {
    let (source, replica) = build_source_and_piecewise_replica()?;
    let expected = source.get_deep_value().to_json_value();
    let replica_vv = replica.oplog_vv();
    assert_ne!(replica_vv, source.oplog_vv());

    let full_updates = source.export(ExportMode::all_updates())?;
    let snapshot = source.export(ExportMode::Snapshot)?;
    let json = source.export_json_updates(&Default::default(), &source.oplog_vv());
    let json_updates = serde_json::to_string(&json).unwrap();

    for (name, import) in [
        (
            "updates",
            Box::new(|doc: &LoroDoc| doc.import(&full_updates).map(|_| ()))
                as Box<dyn Fn(&LoroDoc) -> LoroResult<()>>,
        ),
        (
            "snapshot",
            Box::new(|doc: &LoroDoc| doc.import(&snapshot).map(|_| ())),
        ),
        (
            "batch",
            Box::new(|doc: &LoroDoc| {
                doc.import_batch(&[full_updates.clone(), snapshot.clone()])
                    .map(|_| ())
            }),
        ),
        (
            "json",
            Box::new(|doc: &LoroDoc| doc.import_json_updates(json_updates.as_str()).map(|_| ())),
        ),
    ] {
        let doc = LoroDoc::new();
        doc.import(&replica.export(ExportMode::Snapshot)?)?;
        import(&doc).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(doc.get_deep_value().to_json_value(), expected, "{name}");

        // The replica itself, whose change store holds the history split up.
        let doc = LoroDoc::new();
        doc.import(&replica.export(ExportMode::all_updates())?)?;
        doc.import(&replica.export(ExportMode::updates(&doc.oplog_vv()))?)?;
        import(&doc).unwrap_or_else(|e| panic!("{name} (updates base): {e:?}"));
        assert_eq!(doc.get_deep_value().to_json_value(), expected, "{name}");
    }

    // And in the other direction: the merged history into the piecewise one.
    replica.import(&full_updates)?;
    assert_eq!(replica.get_deep_value().to_json_value(), expected);
    Ok(())
}

/// Two exports of the same peer's history that were cut at different points,
/// imported so that every imported change straddles the local counter end.
#[test]
fn reimporting_overlapping_ranges_of_the_same_history_succeeds() -> LoroResult<()> {
    let source = LoroDoc::new();
    source.set_peer_id(1)?;
    let text = source.get_text("text");
    let mut versions = vec![source.oplog_vv()];
    for i in 0..20 {
        text.insert(text.len_unicode(), &format!("{i},"))?;
        if i % 3 == 0 {
            text.delete(0, 1)?;
        }
        source.commit();
        versions.push(source.oplog_vv());
    }

    let target = LoroDoc::new();
    for window in versions.windows(3) {
        // Export [v_i, v_{i+2}) while the target already has up to v_{i+1}.
        let update = source.export(ExportMode::updates(&window[0]))?;
        target.import(&update)?;
    }
    assert_eq!(
        target.get_deep_value().to_json_value(),
        source.get_deep_value().to_json_value()
    );
    Ok(())
}

/// The issue also noted that dropping the doc aborted the process when the
/// failed import panicked inside `catch_unwind`. With an `Err` there is no panic;
/// make sure dropping inside the closure is fine.
#[test]
fn rejected_import_inside_catch_unwind_can_drop_the_doc() {
    let result = std::panic::catch_unwind(|| {
        let history = LoroDoc::new();
        history.set_peer_id(7).unwrap();
        history.get_map("m").insert("k", 0).unwrap();
        history.get_text("t").insert(0, "a").unwrap();
        history.commit();

        let other = LoroDoc::new();
        other.set_peer_id(7).unwrap();
        other.get_text("t").insert(0, "xyz").unwrap();
        other.commit();

        let r = history.import(&other.export(ExportMode::all_updates()).unwrap());
        drop(history);
        r.is_err()
    });
    assert_eq!(result.ok(), Some(true));
}

/// JSON updates for `doc`'s last change, moved to `peer` at `counter` with `deps`.
fn forged_json_change(doc: &LoroDoc, peer: u64, counter: i32, deps: Vec<ID>) -> loro::JsonSchema {
    let mut json = doc.export_json_updates(&Default::default(), &doc.oplog_vv());
    json.changes.drain(..json.changes.len() - 1);
    let change = json.changes.last_mut().unwrap();
    let shift = counter - change.id.counter;
    change.id.counter = counter;
    change.deps = deps;
    for op in change.ops.iter_mut() {
        op.counter += shift;
    }
    json.peers = Some(vec![peer]);
    change.id.peer = 0;
    json
}

/// A change whose deps are all present but whose own peer's previous counter is
/// not skips counters. It used to trip a DAG assertion under the op log lock,
/// poisoning the doc (and aborting when it was dropped).
#[test]
fn import_change_that_skips_counters_is_rejected() -> LoroResult<()> {
    let writer = LoroDoc::new();
    writer.set_peer_id(8)?;
    writer.get_text("t").insert(0, "xyz")?;
    writer.commit();

    // Into a doc that has none of peer 7, and into one that has 7@0..=1.
    for local_len in [0, 2] {
        let doc = LoroDoc::new();
        doc.set_peer_id(7)?;
        if local_len > 0 {
            doc.get_text("t").insert(0, &"a".repeat(local_len))?;
            doc.commit();
        }
        let before = snapshot_of(&doc);
        let err = doc
            .import_json_updates(forged_json_change(&writer, 7, 5, vec![]))
            .unwrap_err();
        assert!(
            matches!(&err, LoroError::DecodeError(msg) if msg.contains("skips counters")),
            "{err:?}"
        );
        assert_unchanged(&doc, &before);
        doc.set_peer_id(100)?;
        assert_still_usable(&doc);
    }

    // The deps are satisfied by an earlier change of the same import.
    let helper = LoroDoc::new();
    helper.set_peer_id(9)?;
    helper.get_map("m").insert("k", 1)?;
    helper.commit();
    let mut json = helper.export_json_updates(&Default::default(), &helper.oplog_vv());
    let forged = forged_json_change(&writer, 7, 5, vec![ID::new(1, 0)]);
    // Peer index 1 is peer 7, index 0 is peer 9.
    json.peers = Some(vec![9, 7]);
    let mut change = forged.changes.into_iter().next().unwrap();
    change.id.peer = 1;
    change.deps = vec![ID::new(0, 0)];
    json.changes.push(change);

    let doc = LoroDoc::new();
    let before = snapshot_of(&doc);
    let err = doc.import_json_updates(json).unwrap_err();
    assert!(matches!(err, LoroError::DecodeError(_)), "{err:?}");
    assert_unchanged(&doc, &before);
    assert_still_usable(&doc);
    Ok(())
}

/// Known history below a shallow root cannot be compared and must not be
/// rejected for that.
#[test]
fn reimporting_into_a_shallow_doc_still_succeeds() -> LoroResult<()> {
    let source = LoroDoc::new();
    source.set_peer_id(1)?;
    let text = source.get_text("text");
    for i in 0..10 {
        text.insert(0, &format!("{i}"))?;
        source.commit();
    }
    let shallow = LoroDoc::new();
    shallow.import(&source.export(ExportMode::shallow_snapshot(&source.oplog_frontiers()))?)?;
    text.insert(0, "new")?;
    source.commit();

    shallow.import(&source.export(ExportMode::updates(&shallow.shallow_since_vv().to_vv()))?)?;
    assert_eq!(
        shallow.get_deep_value().to_json_value(),
        source.get_deep_value().to_json_value()
    );

    // The full history, which straddles the shallow root, plus one more change.
    text.insert(0, "more")?;
    source.commit();
    shallow.import(&source.export(ExportMode::all_updates())?)?;
    assert_eq!(
        shallow.get_deep_value().to_json_value(),
        source.get_deep_value().to_json_value()
    );
    Ok(())
}
