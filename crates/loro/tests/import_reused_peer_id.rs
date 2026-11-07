//! Importing history whose op ids are already used locally by *different* ops
//! (two clients sharing a peer id) must fail with an `Err` before anything is
//! applied, instead of panicking, poisoning the doc, or scrambling its state.
//! See loro-dev/loro#1118.

use loro::{
    ExportMode, IdSpan, ImportStatus, JsonChange, JsonListOp, JsonMapOp, JsonOpContent, JsonSchema,
    LoroDoc, LoroError, LoroResult, LoroValue, ToJson, TreeParentId, UndoManager, VersionVector,
    ID,
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

    let json = other
        .export_json_updates(&Default::default(), &other.oplog_vv())
        .unwrap();
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

/// A fork that reuses the peer id writes other ops under ids the doc already has.
/// The import brings no new op, and it used to return `Ok` with nothing in
/// `success` or `pending`: the fork's edits were dropped without a word.
fn forked_conflict() -> LoroResult<(LoroDoc, LoroDoc)> {
    let doc = LoroDoc::new();
    doc.set_peer_id(7)?;
    doc.get_text("t").insert(0, "abc")?;
    doc.commit();
    let base = doc.state_frontiers();
    doc.get_text("t").insert(3, "de")?;
    doc.commit();

    let fork = doc.fork_at(&base)?;
    fork.set_peer_id(7)?;
    fork.get_text("t").insert(1, "XY")?;
    fork.commit();
    assert_eq!(fork.oplog_vv(), doc.oplog_vv());
    Ok((doc, fork))
}

#[test]
fn import_with_only_known_conflicting_changes_is_rejected() -> LoroResult<()> {
    let (doc, fork) = forked_conflict()?;
    let before = snapshot_of(&doc);
    let conflict = ID::new(7, 3);

    let updates = fork.export(ExportMode::all_updates())?;
    assert_used_op_id(doc.import(&updates).unwrap_err(), conflict);
    assert_unchanged(&doc, &before);

    let snapshot = fork.export(ExportMode::Snapshot)?;
    assert_used_op_id(doc.import(&snapshot).unwrap_err(), conflict);
    assert_unchanged(&doc, &before);

    let batch = [updates.clone(), snapshot];
    assert_used_op_id(doc.import_batch(&batch).unwrap_err(), conflict);
    assert_unchanged(&doc, &before);

    let json = fork.export_json_updates(&Default::default(), &fork.oplog_vv())?;
    assert_used_op_id(doc.import_json_updates(json).unwrap_err(), conflict);
    assert_unchanged(&doc, &before);

    // Only the tail of the fork, also without the shared prefix in it.
    let mut prefix = fork.oplog_vv();
    prefix.insert(7, 3);
    let tail = fork.export(ExportMode::updates(&prefix))?;
    assert_used_op_id(doc.import(&tail).unwrap_err(), conflict);
    assert_unchanged(&doc, &before);

    assert_still_usable(&doc);
    Ok(())
}

/// A conflict in a doc detached from it, which keeps the state where it is.
#[test]
fn import_with_only_known_conflicting_changes_is_rejected_while_detached() -> LoroResult<()> {
    let (doc, fork) = forked_conflict()?;
    doc.detach();
    let err = doc
        .import(&fork.export(ExportMode::all_updates())?)
        .unwrap_err();
    assert_used_op_id(err, ID::new(7, 3));
    doc.checkout_to_latest();
    assert_eq!(doc.get_text("t").to_string(), "abcde");
    Ok(())
}

/// The sender can only tell what the receiver misses from the receiver's version
/// vector, which already claims these ids. Nothing is sent, so nothing is compared:
/// see "Delta sync" in context/import-peer-id-reuse.md.
#[test]
fn delta_from_the_receivers_version_cannot_reveal_the_conflict() -> LoroResult<()> {
    let (doc, fork) = forked_conflict()?;
    let before = snapshot_of(&doc);
    let status = doc.import(&fork.export(ExportMode::updates(&doc.oplog_vv()))?)?;
    assert!(status.success.is_empty() && status.pending.is_none());
    assert_unchanged(&doc, &before);
    Ok(())
}

/// Importing what the doc already has again and again changes nothing, however the
/// blob was cut and whichever way the history reached this doc.
#[test]
fn reimporting_only_known_history_is_idempotent() -> LoroResult<()> {
    let (source, replica) = build_source_and_piecewise_replica()?;
    replica.import(&source.export(ExportMode::all_updates())?)?;
    let shallow_since = source.oplog_frontiers();
    let mut mid = source.oplog_vv();
    let (&peer, &end) = mid.iter().next().unwrap();
    mid.insert(peer, end / 2);

    let blobs = [
        source.export(ExportMode::all_updates())?,
        replica.export(ExportMode::all_updates())?,
        source.export(ExportMode::updates(&mid))?,
        source.export(ExportMode::Snapshot)?,
        replica.export(ExportMode::Snapshot)?,
        source.export(ExportMode::shallow_snapshot(&shallow_since))?,
    ];
    for doc in [&source, &replica] {
        let before = snapshot_of(doc);
        for _ in 0..2 {
            for blob in &blobs {
                let status = doc.import(blob)?;
                assert!(status.success.is_empty(), "{status:?}");
                assert!(status.pending.is_none());
                assert_unchanged(doc, &before);
            }
            let status = doc.import_batch(&blobs[..5])?;
            assert!(status.success.is_empty(), "{status:?}");
            assert_unchanged(doc, &before);
            let json = source.export_json_updates(&Default::default(), &source.oplog_vv())?;
            let status = doc.import_json_updates(json)?;
            assert!(status.success.is_empty(), "{status:?}");
            assert_unchanged(doc, &before);
        }
    }
    Ok(())
}

/// `1@0`, and updates of two writers that reuse peer 7 on top of it. A receiver
/// without `1@0` parks them.
struct ParkedFork {
    dep: Vec<u8>,
    dep_vv: VersionVector,
    dep_doc: LoroDoc,
}

impl ParkedFork {
    fn new() -> LoroResult<Self> {
        let dep_doc = LoroDoc::new();
        dep_doc.set_peer_id(1)?;
        dep_doc.get_text("t").insert(0, "x")?;
        dep_doc.commit();
        Ok(Self {
            dep: dep_doc.export(ExportMode::all_updates())?,
            dep_vv: dep_doc.oplog_vv(),
            dep_doc,
        })
    }

    fn writer(&self, text: &str) -> LoroResult<LoroDoc> {
        let doc = self.dep_doc.fork();
        doc.set_peer_id(7)?;
        doc.get_text("t").insert(1, text)?;
        doc.commit();
        Ok(doc)
    }

    fn update(&self, text: &str) -> LoroResult<Vec<u8>> {
        Ok(self
            .writer(text)?
            .export(ExportMode::updates(&self.dep_vv))?)
    }
}

fn assert_nothing_new(status: &ImportStatus, pending: bool) {
    assert!(status.success.is_empty(), "{status:?}");
    assert_eq!(status.pending.is_some(), pending, "{status:?}");
}

/// Both updates waited for `1@0`, then the second one was dropped as already known
/// while every import returned `Ok`.
#[test]
fn second_conflicting_update_is_rejected_while_both_wait_for_a_dep() -> LoroResult<()> {
    for (first, second, expected) in [("A", "B", "xA"), ("B", "A", "xB")] {
        let fork = ParkedFork::new()?;
        let (first, second) = (fork.update(first)?, fork.update(second)?);
        let doc = LoroDoc::new();

        assert_nothing_new(&doc.import(&first)?, true);
        assert_used_op_id(doc.import(&second).unwrap_err(), ID::new(7, 0));

        // The rejected update left nothing behind, and the parked one still lands.
        let status = doc.import(&fork.dep)?;
        assert!(status.pending.is_none(), "{status:?}");
        assert_eq!(doc.get_text("t").to_string(), expected);
        assert_used_op_id(doc.import(&second).unwrap_err(), ID::new(7, 0));
        assert_eq!(doc.get_text("t").to_string(), expected);
    }
    Ok(())
}

#[test]
fn parked_update_can_be_delivered_again() -> LoroResult<()> {
    let fork = ParkedFork::new()?;
    let update = fork.update("A")?;
    let doc = LoroDoc::new();
    for _ in 0..3 {
        assert_nothing_new(&doc.import(&update)?, true);
    }
    assert!(doc.import(&fork.dep)?.pending.is_none());
    assert_eq!(doc.get_text("t").to_string(), "xA");
    Ok(())
}

/// Overlaps that do not line up, cut at different change boundaries.
#[test]
fn partly_overlapping_parked_updates_are_compared_over_the_overlap() -> LoroResult<()> {
    let fork = ParkedFork::new()?;
    let writer = fork.writer("ab")?;
    let short = writer.export(ExportMode::updates(&fork.dep_vv))?;
    writer.get_text("t").insert(3, "cd")?;
    writer.commit();
    let long = writer.export(ExportMode::updates(&fork.dep_vv))?;
    let tail = writer.export(ExportMode::updates_in_range(vec![IdSpan::new(7, 1, 4)]))?;

    for order in [
        [&short, &long, &tail],
        [&long, &tail, &short],
        [&tail, &long, &short],
    ] {
        let doc = LoroDoc::new();
        for update in order {
            assert_nothing_new(&doc.import(update)?, true);
        }
        assert!(doc.import(&fork.dep)?.pending.is_none());
        assert_eq!(doc.get_text("t").to_string(), "xabcd");
    }

    // The conflict is at the second op, and only the long update reaches it.
    let other = fork.update("aZ!!")?;
    let doc = LoroDoc::new();
    doc.import(&short)?;
    assert_used_op_id(doc.import(&other).unwrap_err(), ID::new(7, 1));
    doc.import(&fork.dep)?;
    assert_eq!(doc.get_text("t").to_string(), "xab");
    Ok(())
}

/// A change parked on its own peer's previous op, with no other peer involved.
#[test]
fn updates_parked_on_their_own_peer_are_compared() -> LoroResult<()> {
    let fork = ParkedFork::new()?;
    let tail_of = |text: &str| -> LoroResult<(LoroDoc, Vec<u8>)> {
        let doc = fork.writer("a")?;
        let vv = doc.oplog_vv();
        doc.get_text("t").insert(2, text)?;
        doc.commit();
        let update = doc.export(ExportMode::updates(&vv))?;
        Ok((doc, update))
    };
    let (writer, first) = tail_of("B")?;
    let (_, second) = tail_of("C")?;

    let doc = LoroDoc::new();
    doc.import(&fork.dep)?;
    assert_nothing_new(&doc.import(&first)?, true);
    assert_used_op_id(doc.import(&second).unwrap_err(), ID::new(7, 1));
    doc.import(&writer.export(ExportMode::all_updates())?)?;
    assert_eq!(doc.get_text("t").to_string(), "xaB");
    Ok(())
}

/// The second writer needs nothing, so it would apply at once, and the parked first
/// writer would be dropped when its dep arrives.
#[test]
fn update_that_could_apply_is_rejected_when_it_conflicts_with_a_parked_one() -> LoroResult<()> {
    let fork = ParkedFork::new()?;
    let parked = fork.update("A")?;
    let doc = LoroDoc::new();
    doc.import(&parked)?;

    let applicable = LoroDoc::new();
    applicable.set_peer_id(7)?;
    applicable.get_text("t").insert(0, "B")?;
    applicable.commit();
    let updates = applicable.export(ExportMode::all_updates())?;
    assert_used_op_id(doc.import(&updates).unwrap_err(), ID::new(7, 0));
    assert_eq!(doc.oplog_vv(), Default::default());

    doc.import(&fork.dep)?;
    assert_eq!(doc.get_text("t").to_string(), "xA");
    Ok(())
}

/// Bytes that also carry the missing dep are rejected as a whole.
#[test]
fn conflict_with_a_parked_update_is_found_on_every_import_path() -> LoroResult<()> {
    let fork = ParkedFork::new()?;
    let parked = fork.update("A")?;
    let other = fork.writer("B")?;
    let json = other.export_json_updates(&Default::default(), &other.oplog_vv())?;

    let doc = LoroDoc::new();
    doc.import(&parked)?;
    for result in [
        doc.import(&other.export(ExportMode::all_updates())?),
        doc.import(&other.export(ExportMode::Snapshot)?),
        doc.import_batch(&[other.export(ExportMode::all_updates())?]),
        doc.import_json_updates(json),
    ] {
        assert_used_op_id(result.unwrap_err(), ID::new(7, 0));
        assert_eq!(doc.oplog_vv(), Default::default());
    }

    doc.detach();
    let err = doc
        .import(&other.export(ExportMode::all_updates())?)
        .unwrap_err();
    assert_used_op_id(err, ID::new(7, 0));
    doc.attach();

    doc.import(&fork.dep)?;
    assert_eq!(doc.get_text("t").to_string(), "xA");
    Ok(())
}

/// A blob that fails does not undo the other blobs of its batch. The rollback of a
/// failed closing checkout is covered in `import_atomicity`.
#[test]
fn batch_reports_a_conflict_between_its_blobs() -> LoroResult<()> {
    let fork = ParkedFork::new()?;
    let (a, b) = (fork.update("A")?, fork.update("B")?);

    let doc = LoroDoc::new();
    assert_used_op_id(
        doc.import_batch(&[a.clone(), b.clone()]).unwrap_err(),
        ID::new(7, 0),
    );
    assert_used_op_id(doc.import(&b).unwrap_err(), ID::new(7, 0));
    assert!(doc.import(&fork.dep)?.pending.is_none());
    assert_eq!(doc.get_text("t").to_string(), "xA");

    // The dep lands first in the batch and unlocks the parked update.
    let doc = LoroDoc::new();
    doc.import(&a)?;
    assert_used_op_id(
        doc.import_batch(&[fork.dep.clone(), b.clone()])
            .unwrap_err(),
        ID::new(7, 0),
    );
    assert_eq!(doc.get_text("t").to_string(), "xA");
    Ok(())
}

fn assert_write_is_rejected(result: LoroResult<()>, peer: u64) {
    match result {
        Err(LoroError::ConcurrentOpsWithSamePeerID { peer: p, .. }) => assert_eq!(p, peer),
        other => panic!("expected ConcurrentOpsWithSamePeerID, got {other:?}"),
    }
}

/// `7@0..2` waits for `1@0`, and a local write takes `7@0`. The dep used to apply the
/// rest of the parked change over the write: `lamport is not continuous` under the op
/// log lock, which poisoned it and aborted the process on drop.
#[test]
fn local_write_is_rejected_while_parked_changes_hold_its_id() -> LoroResult<()> {
    let parked = ParkedFork::new()?;
    let update = parked.update("ab")?;
    let doc = LoroDoc::new();
    doc.set_peer_id(7)?;
    assert_nothing_new(&doc.import(&update)?, true);

    assert_write_is_rejected(doc.get_map("m").insert("k", 1), 7);
    assert_write_is_rejected(doc.get_text("t").insert(0, "z"), 7);
    doc.commit();
    assert_eq!(doc.oplog_vv().get(&7), None);

    // The dep brings the parked ids in, and the peer can write after them.
    doc.import(&parked.dep)?;
    assert_eq!(doc.get_text("t").to_string(), "xab");
    doc.get_map("m").insert("k", 1)?;
    doc.commit();
    assert_eq!(doc.oplog_vv().get(&7), Some(&3));
    Ok(())
}

#[test]
fn another_peer_can_write_while_parked_changes_hold_the_id() -> LoroResult<()> {
    let parked = ParkedFork::new()?;
    let doc = LoroDoc::new();
    doc.set_peer_id(7)?;
    doc.import(&parked.update("ab")?)?;
    assert_write_is_rejected(doc.get_map("m").insert("k", 1), 7);

    doc.set_peer_id(8)?;
    doc.get_map("m").insert("k", 1)?;
    doc.commit();
    doc.import(&parked.dep)?;
    assert_eq!(doc.get_text("t").to_string(), "xab");
    assert!(doc.get_map("m").get("k").is_some());
    Ok(())
}

/// The parked tail starts past the next id, so a write takes no parked id at first.
/// It still is another history than the one the tail continues.
#[test]
fn local_write_is_rejected_below_a_parked_tail() -> LoroResult<()> {
    let parked = ParkedFork::new()?;
    let writer = parked.writer("abcd")?;
    let tail = writer.export(ExportMode::updates_in_range(vec![IdSpan::new(7, 2, 5)]))?;
    let doc = LoroDoc::new();
    doc.set_peer_id(7)?;
    assert_nothing_new(&doc.import(&tail)?, true);

    assert_write_is_rejected(doc.get_map("m").insert("k", 1), 7);
    Ok(())
}

/// `1@0` waits for `7@0`, which a local write would fill with something else.
#[test]
fn local_write_is_rejected_when_a_parked_change_waits_for_its_id() -> LoroResult<()> {
    let first = LoroDoc::new();
    first.set_peer_id(7)?;
    first.get_text("t").insert(0, "x")?;
    first.commit();
    let dep = first.export(ExportMode::all_updates())?;
    let second = first.fork();
    second.set_peer_id(1)?;
    second.get_text("t").insert(1, "y")?;
    second.commit();
    let update = second.export(ExportMode::updates(&first.oplog_vv()))?;

    let doc = LoroDoc::new();
    doc.set_peer_id(7)?;
    assert_nothing_new(&doc.import(&update)?, true);
    assert_write_is_rejected(doc.get_map("m").insert("k", 1), 7);

    doc.import(&dep)?;
    assert_eq!(doc.get_text("t").to_string(), "xy");
    Ok(())
}

/// `9@0` depends on `5@0` and `7@0`, and sits in the slot of only one of them. A write
/// under the other one used to succeed. The dep of the slot then applied `9@0` on top
/// of it, which left the same version vector as the source and different values,
/// with no error anywhere.
#[test]
fn local_write_is_rejected_under_every_dep_of_a_parked_change() -> LoroResult<()> {
    let writer = |peer: u64, key: &str| -> LoroResult<LoroDoc> {
        let doc = LoroDoc::new();
        doc.set_peer_id(peer)?;
        doc.get_map("m").insert(key, peer as i64)?;
        doc.commit();
        Ok(doc)
    };
    let (five, seven) = (writer(5, "a")?, writer(7, "b")?);
    let source = LoroDoc::new();
    source.set_peer_id(9)?;
    source.import(&five.export(ExportMode::all_updates())?)?;
    source.import(&seven.export(ExportMode::all_updates())?)?;
    source.get_map("m").insert("x", 9)?;
    source.commit();
    let update = source.export(ExportMode::updates_in_range(vec![IdSpan::new(9, 0, 1)]))?;

    let doc = LoroDoc::new();
    assert_nothing_new(&doc.import(&update)?, true);
    for peer in [5, 7] {
        doc.set_peer_id(peer)?;
        assert_write_is_rejected(doc.get_map("m").insert("k", 1), peer);
    }

    // One dep lets peer 5 write again, and the other one brings the change in.
    doc.import(&five.export(ExportMode::all_updates())?)?;
    doc.set_peer_id(5)?;
    doc.get_map("n").insert("k", 1)?;
    doc.commit();
    doc.set_peer_id(7)?;
    assert_write_is_rejected(doc.get_map("n").insert("k", 2), 7);
    doc.import(&seven.export(ExportMode::all_updates())?)?;
    assert_eq!(
        doc.get_map("m").get_deep_value().to_json_value(),
        source.get_map("m").get_deep_value().to_json_value()
    );
    Ok(())
}

/// Every path to a local commit reports it, and none of them hurts the doc.
#[test]
fn writes_of_every_kind_report_parked_ids() -> LoroResult<()> {
    let parked = ParkedFork::new()?;
    let doc = LoroDoc::new();
    doc.set_peer_id(7)?;
    let mut undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    doc.get_text("t").insert(0, "a")?;
    doc.commit();
    let before_write = doc.oplog_frontiers();
    doc.get_text("t").insert(1, "b")?;
    doc.commit();
    let base = doc.state_frontiers();

    // A writer that shares the history so far writes on top of `1@0`, which `doc` lacks.
    let writer = doc.fork();
    writer.import(&parked.dep)?;
    writer.set_peer_id(7)?;
    writer.get_text("t").insert(1, "c")?;
    writer.commit();
    let update = writer.export(ExportMode::updates_in_range(vec![IdSpan::new(7, 2, 3)]))?;
    assert_nothing_new(&doc.import(&update)?, true);

    let before = snapshot_of(&doc);
    assert_write_is_rejected(doc.revert_to(&before_write), 7);
    assert_unchanged(&doc, &before);
    let diff = doc.diff(&base, &before_write)?;
    assert_write_is_rejected(doc.apply_diff(diff), 7);
    assert_unchanged(&doc, &before);
    assert!(matches!(
        undo.undo(),
        Err(LoroError::ConcurrentOpsWithSamePeerID { peer: 7, .. })
    ));
    assert_unchanged(&doc, &before);

    doc.commit();
    doc.import(&parked.dep)?;
    doc.get_map("m").insert("k", 1)?;
    doc.commit();
    assert_eq!(doc.oplog_vv().get(&7), Some(&4));
    Ok(())
}

fn map_change(peer: u64, counter: i32, deps: Vec<ID>, values: &[LoroValue]) -> JsonChange {
    let source = LoroDoc::new();
    source.set_peer_id(peer).unwrap();
    for i in 0..values.len() {
        source.get_map("m").insert("k", i as i64).unwrap();
    }
    source.commit();
    let mut json = source
        .export_json_updates_without_peer_compression(&Default::default(), &source.oplog_vv())
        .unwrap();
    let mut change = json.changes.remove(0);
    change.id = ID::new(peer, counter);
    change.deps = deps;
    for (i, op) in change.ops.iter_mut().enumerate() {
        op.counter = counter + i as i32;
        if let JsonOpContent::Map(JsonMapOp::Insert { value, .. }) = &mut op.content {
            *value = values[i].clone();
        }
    }
    change
}

fn json_of(changes: Vec<JsonChange>) -> JsonSchema {
    JsonSchema {
        schema_version: 1,
        start_version: Default::default(),
        peers: None,
        changes,
    }
}

/// Decodes fine and fails when applied to the state: a list insert past the end. The
/// import is rolled back, and it says nothing about anything else in the doc.
fn out_of_bounds_list_insert(peer: u64) -> JsonSchema {
    let source = LoroDoc::new();
    source.set_peer_id(peer).unwrap();
    for i in 0..6 {
        source.get_list("l").insert(i, 0).unwrap();
    }
    source.commit();
    let mut json = source
        .export_json_updates_without_peer_compression(&Default::default(), &source.oplog_vv())
        .unwrap();
    let mut change = json.changes.remove(0);
    let mut op = change.ops.remove(0);
    if let JsonOpContent::List(JsonListOp::Insert { pos, .. }) = &mut op.content {
        *pos = 5;
    }
    change.ops = vec![op];
    json_of(vec![change])
}

/// Null matches NaN and infinity, which do not match each other, so the three are
/// only all accepted while null holds the id. The suffixes wait for `7@0`, and the
/// holder for `1@0`.
fn lossy_values_over_two_slots(doc: &LoroDoc) -> LoroResult<JsonChange> {
    let suffix = |value: f64| map_change(7, 1, vec![ID::new(7, 0)], &[LoroValue::Double(value)]);
    let holder = map_change(
        7,
        0,
        vec![ID::new(1, 0)],
        &[LoroValue::Null, LoroValue::Null],
    );
    doc.import_json_updates(json_of(vec![holder]))?;
    doc.import_json_updates(json_of(vec![suffix(f64::NAN)]))?;
    doc.import_json_updates(json_of(vec![suffix(f64::INFINITY)]))?;
    Ok(suffix(f64::INFINITY))
}

#[test]
fn failed_import_keeps_which_parked_change_holds_an_id() -> LoroResult<()> {
    let parked = ParkedFork::new()?;
    let doc = LoroDoc::new();
    let accepted = lossy_values_over_two_slots(&doc)?;

    doc.import_json_updates(out_of_bounds_list_insert(3))
        .unwrap_err();
    assert_nothing_new(
        &doc.import_json_updates(json_of(vec![accepted.clone()]))?,
        true,
    );

    doc.import(&parked.dep)?;
    assert_eq!(doc.oplog_vv().get(&7), Some(&2));
    assert!(doc.get_map("m").get("k").is_some());
    Ok(())
}

/// The failed import also got as far as releasing the holder's ids: it brought the
/// dep that unlocks the holder, and then failed on a change that came with it.
#[test]
fn failed_import_that_unlocked_a_holder_keeps_it() -> LoroResult<()> {
    let parked = ParkedFork::new()?;
    let doc = LoroDoc::new();
    let accepted = lossy_values_over_two_slots(&doc)?;

    let dep = parked
        .dep_doc
        .export_json_updates_without_peer_compression(
            &Default::default(),
            &parked.dep_doc.oplog_vv(),
        )?;
    let mut failing = out_of_bounds_list_insert(3);
    failing.changes.extend(dep.changes);
    let before = doc.oplog_vv();
    doc.import_json_updates(failing).unwrap_err();
    assert_eq!(doc.oplog_vv(), before);

    // Checked first: an update accepted here would hold the ids itself. The holder is
    // back, not just out of the way, so what it rejects stays rejected.
    let different = map_change(7, 1, vec![ID::new(7, 0)], &[LoroValue::I64(9)]);
    match doc.import_json_updates(json_of(vec![different])) {
        Err(LoroError::UsedOpID { id }) => assert_eq!(id, ID::new(7, 1)),
        other => panic!("expected UsedOpID, got {other:?}"),
    }
    assert_nothing_new(
        &doc.import_json_updates(json_of(vec![accepted.clone()]))?,
        true,
    );
    doc.set_peer_id(7)?;
    assert_write_is_rejected(doc.get_map("m").insert("k", 1), 7);
    doc.import(&parked.dep)?;
    assert_eq!(doc.oplog_vv().get(&7), Some(&2));
    Ok(())
}

/// What a failed import parked leaves with it, so it holds nothing afterwards.
#[test]
fn failed_import_leaves_no_claims_behind() -> LoroResult<()> {
    let parked = ParkedFork::new()?;
    let failing = || -> LoroResult<JsonSchema> {
        let mut failing = out_of_bounds_list_insert(3);
        failing
            .changes
            .push(map_change(8, 0, vec![ID::new(1, 5)], &[LoroValue::I64(1)]));
        let dep = &parked.dep_doc;
        let json =
            dep.export_json_updates_without_peer_compression(&Default::default(), &dep.oplog_vv())?;
        failing.changes.extend(json.changes);
        Ok(failing)
    };

    // Another update can take the ids.
    let doc = LoroDoc::new();
    doc.import_json_updates(failing()?).unwrap_err();
    let other = map_change(8, 0, vec![ID::new(1, 5)], &[LoroValue::I64(2)]);
    assert_nothing_new(&doc.import_json_updates(json_of(vec![other]))?, true);

    // So can a local write, also under the dep it had.
    let doc = LoroDoc::new();
    doc.import_json_updates(failing()?).unwrap_err();
    for peer in [8, 1] {
        doc.set_peer_id(peer)?;
        doc.get_map("m").insert("k", peer as i64)?;
        doc.commit();
        assert_eq!(doc.oplog_vv().get(&peer), Some(&1));
    }
    Ok(())
}

/// A failed import does not free what stays parked for local writes either.
#[test]
fn failed_import_keeps_parked_ids_from_local_writes() -> LoroResult<()> {
    let parked = ParkedFork::new()?;
    let doc = LoroDoc::new();
    doc.set_peer_id(7)?;
    doc.import(&parked.update("ab")?)?;
    doc.import_json_updates(out_of_bounds_list_insert(3))
        .unwrap_err();
    assert_write_is_rejected(doc.get_map("m").insert("k", 1), 7);
    doc.import(&parked.dep)?;
    assert_eq!(doc.get_text("t").to_string(), "xab");
    Ok(())
}

/// One import applied the first change and parked the second over it, so `1@0`
/// brought the second over the first and tripped the DAG assertion under the op log
/// lock. Nothing is parked or applied now.
#[test]
fn import_that_contradicts_itself_is_rejected() -> LoroResult<()> {
    let parked = ParkedFork::new()?;
    let direct = map_change(7, 0, vec![], &[LoroValue::I64(1)]);
    let over = map_change(
        7,
        0,
        vec![ID::new(1, 0)],
        &[LoroValue::I64(2), LoroValue::I64(3), LoroValue::I64(4)],
    );
    for changes in [
        vec![direct.clone(), over.clone()],
        vec![over.clone(), direct.clone()],
    ] {
        let doc = LoroDoc::new();
        match doc.import_json_updates(json_of(changes)) {
            Err(LoroError::UsedOpID { id }) => assert_eq!(id, ID::new(7, 0)),
            other => panic!("expected UsedOpID, got {other:?}"),
        }
        assert_eq!(doc.oplog_vv().get(&7), None);
        doc.import(&parked.dep)?;
        assert_eq!(doc.get_text("t").to_string(), "x");
    }
    Ok(())
}

#[test]
fn import_with_changes_out_of_order_that_agree_is_accepted() -> LoroResult<()> {
    let values = [LoroValue::I64(1), LoroValue::I64(2), LoroValue::I64(3)];
    let whole = map_change(7, 0, vec![], &values);
    let tail = map_change(7, 1, vec![ID::new(7, 0)], &values[1..]);
    let doc = LoroDoc::new();
    let status = doc.import_json_updates(json_of(vec![tail, whole]))?;
    assert_eq!(status.pending, None);
    assert_eq!(doc.oplog_vv().get(&7), Some(&3));
    assert!(doc.get_map("m").get("k").is_some());
    Ok(())
}

/// Pending changes, local commits and rollback in every order. Whatever each step
/// returns, the doc must end up usable and agree with a replay of its own history.
#[test]
fn parked_changes_local_writes_and_rollback_in_every_order() -> LoroResult<()> {
    fn permutations(items: &[usize]) -> Vec<Vec<usize>> {
        if items.len() <= 1 {
            return vec![items.to_vec()];
        }
        let mut all = Vec::new();
        for (i, &first) in items.iter().enumerate() {
            let mut rest = items.to_vec();
            rest.remove(i);
            for mut tail in permutations(&rest) {
                tail.insert(0, first);
                all.push(tail);
            }
        }
        all
    }

    let parked = ParkedFork::new()?;
    let update = parked.update("ab")?;
    let conflicting = {
        let doc = LoroDoc::new();
        doc.set_peer_id(7)?;
        doc.get_map("m").insert("k", 1)?;
        doc.commit();
        doc.export(ExportMode::all_updates())?
    };

    // A detached import does not touch the state, so step 4 cannot fail there.
    for (detached, steps) in [
        (false, &[0, 1, 2, 3, 4, 5][..]),
        (true, &[0, 1, 2, 3, 5][..]),
    ] {
        for order in permutations(steps) {
            let doc = LoroDoc::new();
            if detached {
                doc.set_detached_editing(true);
                doc.detach();
            }
            doc.set_peer_id(7)?;
            for &step in &order {
                let _ = match step {
                    0 | 5 => doc.import(&update).map(|_| ()),
                    1 => doc.import(&conflicting).map(|_| ()),
                    2 => doc.import(&parked.dep).map(|_| ()),
                    3 => doc.get_map("m").insert("local", 1),
                    _ => doc
                        .import_json_updates(out_of_bounds_list_insert(3))
                        .map(|_| ()),
                };
                doc.commit();
            }

            doc.import(&update).ok();
            doc.import(&parked.dep).ok();
            doc.checkout_to_latest();
            let replay = LoroDoc::new();
            replay.import(&doc.export(ExportMode::all_updates())?)?;
            // Asking for a root makes it visible, also while it is empty.
            for doc in [&replay, &doc] {
                doc.get_map("m");
                doc.get_list("l");
            }
            assert_eq!(
                replay.get_deep_value().to_json_value(),
                doc.get_deep_value().to_json_value(),
                "{order:?} detached={detached}"
            );
        }
    }
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
    let json = source
        .export_json_updates(&Default::default(), &source.oplog_vv())
        .unwrap();
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
    let mut json = doc
        .export_json_updates(&Default::default(), &doc.oplog_vv())
        .unwrap();
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
    let mut json = helper
        .export_json_updates(&Default::default(), &helper.oplog_vv())
        .unwrap();
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

/// Exercise many cold blocks, a merged update, and the snapshot scratch arena.
/// The first mismatch must still name the exact atom far inside the prefix.
#[test]
fn cold_history_conflict_inside_a_large_known_prefix_is_rejected() -> LoroResult<()> {
    let history = LoroDoc::new();
    let other = LoroDoc::new();
    for doc in [&history, &other] {
        doc.set_peer_id(7)?;
        doc.set_change_merge_interval(-1);
    }
    for i in 0..3000 {
        history.get_text("t").insert(i, "a")?;
        history.commit();
        other
            .get_text("t")
            .insert(i, if i == 2000 { "O" } else { "a" })?;
        other.commit();
    }
    other.get_text("t").insert(3000, "Z")?;
    other.commit();
    let base = history.export(ExportMode::Snapshot)?;
    for blob in [
        other.export(ExportMode::all_updates())?,
        other.export(ExportMode::Snapshot)?,
    ] {
        for batch in [false, true] {
            let target = LoroDoc::new();
            target.import(&base)?;
            let before = snapshot_of(&target);
            let err = if batch {
                target.import_batch(&[blob.clone()]).unwrap_err()
            } else {
                target.import(&blob).unwrap_err()
            };
            assert_used_op_id(err, ID::new(7, 2000));
            assert_unchanged(&target, &before);
            assert_still_usable(&target);
        }
    }
    Ok(())
}

/// The same history can arrive as one merged Text insert instead of thousands
/// of small changes. Its blocks differ from the snapshot's cold history, but
/// the overlap is equal and its new suffix must be accepted.
#[test]
fn cold_text_overlap_with_merged_updates_is_accepted() -> LoroResult<()> {
    let history = LoroDoc::new();
    history.set_peer_id(7)?;
    history.set_change_merge_interval(-1);
    for i in 0..3000 {
        history.get_text("t").insert(i * 2, "a😀")?;
        history.commit();
    }

    let merged = LoroDoc::new();
    merged.set_peer_id(7)?;
    merged
        .get_text("t")
        .insert(0, &("a😀".repeat(3000) + "Z"))?;
    merged.commit();

    let target = LoroDoc::new();
    target.import(&history.export(ExportMode::Snapshot)?)?;
    target.import(&merged.export(ExportMode::all_updates())?)?;
    assert_eq!(target.get_deep_value(), merged.get_deep_value());
    assert_eq!(target.oplog_vv(), merged.oplog_vv());
    assert_eq!(target.state_frontiers(), merged.state_frontiers());
    Ok(())
}
