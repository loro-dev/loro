//! `ExportMode::updates_in_range` with several spans of one peer, which used to
//! panic with "counter should be continuous" while holding the op log lock.
//! See loro-dev/loro#1155.

use loro::{ExportMode, IdSpan, LoroDoc, ToJson, VersionVector};
use pretty_assertions::assert_eq;

fn source() -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("t");
    for i in 0..10 {
        text.insert(i, "a").unwrap();
        doc.commit();
    }
    let other = LoroDoc::new();
    other.set_peer_id(2).unwrap();
    other
        .import(&doc.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    for i in 0..6 {
        other.get_map("m").insert(&format!("k{i}"), i).unwrap();
        other.commit();
    }
    doc.import(&other.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    doc
}

fn vv(entries: &[(u64, i32)]) -> VersionVector {
    let mut vv = VersionVector::new();
    for &(peer, end) in entries {
        vv.insert(peer, end);
    }
    vv
}

#[test]
fn issue_repro_non_contiguous_spans_of_one_peer() {
    let doc = source();
    let bytes = doc
        .export(ExportMode::updates_in_range(vec![
            IdSpan::new(1, 0, 2),
            IdSpan::new(1, 5, 7),
        ]))
        .unwrap();

    // A doc that has 1@0..5 gets both ranges; a fresh one gets the first and parks
    // the second until 1@2..5 arrive.
    let has_prefix = LoroDoc::new();
    has_prefix
        .import(
            &doc.export(ExportMode::updates_in_range(vec![IdSpan::new(1, 0, 5)]))
                .unwrap(),
        )
        .unwrap();
    has_prefix.import(&bytes).unwrap();
    assert_eq!(has_prefix.oplog_vv(), vv(&[(1, 7)]));

    let fresh = LoroDoc::new();
    let status = fresh.import(&bytes).unwrap();
    assert_eq!(fresh.oplog_vv(), vv(&[(1, 2)]));
    assert!(status.pending.is_some());
    fresh
        .import(
            &doc.export(ExportMode::updates_in_range(vec![IdSpan::new(1, 2, 5)]))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(fresh.oplog_vv(), vv(&[(1, 7)]));
}

#[test]
fn overlapping_unordered_spans_of_several_peers() {
    let doc = source();
    let bytes = doc
        .export(ExportMode::updates_in_range(vec![
            IdSpan::new(1, 6, 10),
            IdSpan::new(2, 3, 6),
            IdSpan::new(1, 3, 7),
            IdSpan::new(1, 0, 3),
            IdSpan::new(2, 0, 1),
            IdSpan::new(2, 1, 2),
        ]))
        .unwrap();
    let fresh = LoroDoc::new();
    fresh.import(&bytes).unwrap();
    // 1@0..10 as one range, 2@0..2 and 2@3..6 (2@2 missing, so 2@3.. stays pending).
    assert_eq!(fresh.oplog_vv(), vv(&[(1, 10), (2, 2)]));
    fresh
        .import(
            &doc.export(ExportMode::updates_in_range(vec![IdSpan::new(2, 2, 3)]))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(fresh.oplog_vv(), doc.oplog_vv());
    assert_eq!(
        fresh.get_deep_value().to_json_value(),
        doc.get_deep_value().to_json_value()
    );
}

#[test]
fn many_small_spans_round_trip() {
    let doc = source();
    let spans: Vec<IdSpan> = (0..10)
        .step_by(2)
        .map(|i| IdSpan::new(1, i, i + 1))
        .collect();
    let odd: Vec<IdSpan> = (1..10)
        .step_by(2)
        .map(|i| IdSpan::new(1, i, i + 1))
        .collect();
    let fresh = LoroDoc::new();
    fresh
        .import(&doc.export(ExportMode::updates_in_range(spans)).unwrap())
        .unwrap();
    fresh
        .import(&doc.export(ExportMode::updates_in_range(odd)).unwrap())
        .unwrap();
    assert_eq!(fresh.oplog_vv(), vv(&[(1, 10)]));
    assert_eq!(
        fresh.get_text("t").to_string(),
        doc.get_text("t").to_string()
    );
}
