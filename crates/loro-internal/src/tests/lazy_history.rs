//! An import into a document opened from a snapshot should not load its whole history.

use crate::{
    loro::ExportMode,
    version::{Frontiers, VersionVector},
    LoroDoc,
};

const TURNS: u64 = 300;

/// A history where two peers take turns, so every change is its own DAG node.
fn alternating_history() -> LoroDoc {
    let doc = LoroDoc::new_auto_commit();
    for i in 0..TURNS {
        append_as(&doc, 1 + i % 2, "x");
    }
    doc
}

fn append_as(doc: &LoroDoc, peer: u64, text: &str) {
    doc.start_auto_commit();
    doc.set_peer_id(peer).unwrap();
    doc.get_text("text")
        .insert(0, text, crate::cursor::PosType::Unicode)
        .unwrap();
    doc.commit_then_renew();
}

fn parsed_dag_nodes(doc: &LoroDoc) -> usize {
    doc.oplog().lock().dag.total_parsed_dag_node()
}

fn cold_copy(snapshot: &[u8]) -> LoroDoc {
    LoroDoc::from_snapshot(snapshot).unwrap()
}

#[test]
fn cold_sequential_import_loads_only_the_dag_nodes_near_the_head() {
    let snapshot = alternating_history().export(ExportMode::Snapshot).unwrap();
    let source = cold_copy(&snapshot);
    let base = source.oplog_vv();
    append_as(&source, 7, "remote");
    let update = source.export(ExportMode::updates(&base)).unwrap();

    let cold = cold_copy(&snapshot);
    cold.import(&update).unwrap();

    assert!(
        parsed_dag_nodes(&cold) < TURNS as usize / 10,
        "one small import loaded {} of {TURNS} DAG nodes",
        parsed_dag_nodes(&cold),
    );
    assert_eq!(cold.get_deep_value(), source.get_deep_value());
}

/// The version an import reports for its new head has to be the one a full walk finds.
fn assert_head_vv_matches_a_full_walk(doc: &LoroDoc) {
    let head: Frontiers = doc.oplog_frontiers();
    let walked: VersionVector = cold_copy(&doc.export(ExportMode::Snapshot).unwrap())
        .frontiers_to_vv(&head)
        .unwrap();
    assert_eq!(doc.frontiers_to_vv(&head).unwrap(), walked);
    assert_eq!(walked, doc.oplog_vv());
}

#[test]
fn chained_cold_imports_keep_correct_version_vectors() {
    let snapshot = alternating_history().export(ExportMode::Snapshot).unwrap();
    let source = cold_copy(&snapshot);
    let cold = cold_copy(&snapshot);

    for (peer, text) in [(7, "a"), (8, "b"), (7, "c")] {
        let base = source.oplog_vv();
        append_as(&source, peer, text);
        cold.import(&source.export(ExportMode::updates(&base)).unwrap())
            .unwrap();
        assert_head_vv_matches_a_full_walk(&cold);
    }
    assert_eq!(cold.get_deep_value(), source.get_deep_value());
}

#[test]
fn cold_import_on_top_of_two_heads_keeps_correct_version_vectors() {
    let history = alternating_history();
    let left = cold_copy(&history.export(ExportMode::Snapshot).unwrap());
    let right = cold_copy(&history.export(ExportMode::Snapshot).unwrap());
    append_as(&left, 7, "left");
    append_as(&right, 8, "right");
    left.import(&right.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(left.oplog_frontiers().len(), 2);

    let snapshot = left.export(ExportMode::Snapshot).unwrap();
    let source = cold_copy(&snapshot);
    let base = source.oplog_vv();
    append_as(&source, 9, "on both");
    let update = source.export(ExportMode::updates(&base)).unwrap();

    let cold = cold_copy(&snapshot);
    cold.import(&update).unwrap();

    assert_head_vv_matches_a_full_walk(&cold);
    assert_eq!(cold.get_deep_value(), source.get_deep_value());
}

#[test]
fn checkout_after_a_cold_import_sees_the_old_version() {
    let snapshot = alternating_history().export(ExportMode::Snapshot).unwrap();
    let source = cold_copy(&snapshot);
    let before = source.oplog_frontiers();
    let expected = source.get_deep_value();
    let base = source.oplog_vv();
    append_as(&source, 7, "remote");

    let cold = cold_copy(&snapshot);
    cold.import(&source.export(ExportMode::updates(&base)).unwrap())
        .unwrap();
    cold.checkout(&before).unwrap();

    assert_eq!(cold.get_deep_value(), expected);
}

#[test]
fn a_zero_counter_in_the_start_vv_keeps_the_empty_replay_base_critical() {
    let doc = alternating_history();
    let oplog = doc.oplog().lock();
    let mut from = VersionVector::default();
    from.insert(99, 0);

    let (base, _) = oplog
        .try_iter_from_replay_base_causally(
            &from,
            &Frontiers::default(),
            oplog.vv(),
            oplog.frontiers(),
        )
        .unwrap();

    assert!(base.is_critical);
}

#[test]
fn cold_two_head_transitive_predecessor_loads_only_head_dag_nodes() {
    let history = alternating_history();
    let left = cold_copy(&history.export(ExportMode::Snapshot).unwrap());
    let right = cold_copy(&history.export(ExportMode::Snapshot).unwrap());
    append_as(&left, 7, "left");
    append_as(&right, 8, "right");

    let left_branch = cold_copy(&left.export(ExportMode::Snapshot).unwrap());
    let v0 = left_branch.oplog_vv();
    append_as(&left_branch, 9, "bridge");
    let bridge_update = left_branch.export(ExportMode::updates(&v0)).unwrap();
    let v1 = left_branch.oplog_vv();
    append_as(&left_branch, 7, "left-second");
    let follow_update = left_branch.export(ExportMode::updates(&v1)).unwrap();

    left.import(&right.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let two_head_snapshot = left.export(ExportMode::Snapshot).unwrap();

    // Detached import isolates OpLog / DAG loading from state checkout replay:
    // `bridge_update` puts `0@9` into `vv` alongside `0@8`, so `1@7` (`deps = [0@9]`)
    // resolves `0@9 -> 0@7` inside the cold two-headed DAG without loading the 300-turn base.
    let cold = cold_copy(&two_head_snapshot);
    cold.detach();
    cold.import(&bridge_update).unwrap();
    cold.import(&follow_update).unwrap();
    assert!(
        parsed_dag_nodes(&cold) < TURNS as usize / 10,
        "transitive predecessor check on two-headed cold snapshot loaded {} of {TURNS} DAG nodes",
        parsed_dag_nodes(&cold),
    );
}
