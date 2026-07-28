use std::borrow::Cow;

use super::gen_action;
use loro::{ExportMode, LoroDoc};

#[test]
fn test_snapshot_at_with_multiple_actions() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;

    // Perform a series of actions
    gen_action(&doc, 1, 10);
    doc.commit();
    let frontiers_after_first_commit = doc.oplog_frontiers();
    let value_after_first_commit = doc.get_deep_value();

    gen_action(&doc, 2, 20);
    doc.commit();
    let frontiers_after_second_commit = doc.oplog_frontiers();
    let value_after_second_commit = doc.get_deep_value();
    // Export snapshot at the first frontiers
    let snapshot_at_first = doc.export(ExportMode::SnapshotAt {
        version: Cow::Borrowed(&frontiers_after_first_commit),
    });
    let new_doc_first = LoroDoc::new();
    new_doc_first.import(&snapshot_at_first.unwrap())?;

    // Verify the state of the new document matches the expected state
    assert_eq!(new_doc_first.get_deep_value(), value_after_first_commit);

    // Export snapshot at the second frontiers
    let snapshot_at_second = doc.export(ExportMode::SnapshotAt {
        version: Cow::Borrowed(&frontiers_after_second_commit),
    });
    let new_doc_second = LoroDoc::new();
    new_doc_second.import(&snapshot_at_second.unwrap())?;

    // Verify the state of the new document matches the expected state
    assert_eq!(new_doc_second.get_deep_value(), value_after_second_commit);

    Ok(())
}

#[test]
fn test_fork_at_target_frontiers() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;

    // Perform initial actions
    gen_action(&doc, 1, 10);
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    let value_after_first_commit = doc.get_deep_value();

    // Perform more actions
    gen_action(&doc, 2, 20);
    doc.commit();

    let new_doc = doc.fork_at(&frontiers)?;
    assert_eq!(new_doc.get_deep_value(), value_after_first_commit);

    // Import all updates to the new document
    new_doc.import(&doc.export(ExportMode::all_updates()).unwrap())?;
    assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());

    Ok(())
}

/// Regression for loro-dev/loro#1056.
///
/// When the current frontiers hold a concurrent branch and the target version is a
/// strict subset of them, the target itself is the maximal common ancestor, so the
/// diff calculator replays from there instead of from the start of history. Assert
/// that the tighter replay base still produces the right state in both directions.
#[test]
fn test_fork_at_with_concurrent_frontiers() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 1, 20);
    doc.commit();

    // A second peer branches off before peer 1 keeps editing.
    let other = LoroDoc::new();
    other.import(&doc.export(ExportMode::Snapshot).unwrap())?;
    other.set_peer_id(2)?;

    gen_action(&doc, 2, 30);
    doc.commit();
    let target = doc.oplog_frontiers();
    let value_at_target = doc.get_deep_value();

    gen_action(&other, 3, 10);
    other.commit();
    doc.import(&other.export(ExportMode::all_updates()).unwrap())?;
    assert_eq!(
        doc.state_frontiers().len(),
        2,
        "the merged frontiers should keep both branches"
    );
    let merged_value = doc.get_deep_value();

    let forked = doc.fork_at(&target)?;
    assert_eq!(forked.get_deep_value(), value_at_target);
    // `fork_at` checks out internally; the source doc must be left untouched.
    assert_eq!(doc.state_frontiers(), doc.oplog_frontiers());
    assert_eq!(doc.get_deep_value(), merged_value);

    // The same ancestor logic drives checkout and re-attach.
    doc.checkout(&target)?;
    assert_eq!(doc.get_deep_value(), value_at_target);
    doc.attach();
    assert_eq!(doc.get_deep_value(), merged_value);

    forked.import(&doc.export(ExportMode::all_updates()).unwrap())?;
    assert_eq!(forked.get_deep_value(), merged_value);

    Ok(())
}
