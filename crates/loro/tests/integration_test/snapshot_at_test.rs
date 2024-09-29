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
    new_doc_first.import(&snapshot_at_first)?;

    // Verify the state of the new document matches the expected state
    assert_eq!(new_doc_first.get_deep_value(), value_after_first_commit);

    // Export snapshot at the second frontiers
    let snapshot_at_second = doc.export(ExportMode::SnapshotAt {
        version: Cow::Borrowed(&frontiers_after_second_commit),
    });
    let new_doc_second = LoroDoc::new();
    new_doc_second.import(&snapshot_at_second)?;

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

    let new_doc = doc.fork_at(&frontiers);
    assert_eq!(new_doc.get_deep_value(), value_after_first_commit);

    // Import all updates to the new document
    new_doc.import(&doc.export(ExportMode::all_updates()))?;
    assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());

    Ok(())
}
