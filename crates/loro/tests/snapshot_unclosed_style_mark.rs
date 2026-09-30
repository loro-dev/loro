//! A version can hold a mark's `StyleStart` op but not its `StyleEnd` (op-level
//! frontiers: the two ops have consecutive counters). `checkout` handles it; a
//! snapshot of it used to panic on the first read with "unclosed style mark".
//! See loro-dev/loro#1165.

use loro::{ExpandType, ExportMode, Frontiers, LoroDoc, StyleConfig, StyleConfigMap, ToJson, ID};
use pretty_assertions::assert_eq;

fn source() -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let mut styles = StyleConfigMap::new();
    styles.insert(
        "bold".into(),
        StyleConfig {
            expand: ExpandType::After,
        },
    );
    doc.config_text_style(styles);
    let text = doc.get_text("t");
    text.insert(0, "abc").unwrap(); // 0@1..=2@1
    text.mark(1..3, "bold", true).unwrap(); // StyleStart 3@1, StyleEnd 4@1
    text.insert(3, "Z").unwrap(); // 5@1
    doc.commit();
    doc
}

fn mid_mark() -> Frontiers {
    Frontiers::from_id(ID::new(1, 3))
}

fn checkout_delta(doc: &LoroDoc, f: &Frontiers) -> serde_json::Value {
    let tmp = doc.fork();
    tmp.checkout(f).unwrap();
    tmp.get_text("t").get_richtext_value().to_json_value()
}

#[test]
fn fork_at_version_with_unclosed_mark_is_readable() {
    let doc = source();
    let expected = checkout_delta(&doc, &mid_mark());
    let fork = doc.fork_at(&mid_mark()).unwrap();
    assert_eq!(
        fork.get_text("t").get_richtext_value().to_json_value(),
        expected
    );
    assert_eq!(fork.get_text("t").to_string(), "abc");

    // The fork keeps working: edits, and the rest of the history closes the mark.
    fork.get_text("t").insert(0, ">").unwrap();
    fork.commit();
    fork.import(&doc.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    doc.import(&fork.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(
        fork.get_text("t").get_richtext_value().to_json_value(),
        doc.get_text("t").get_richtext_value().to_json_value()
    );
}

#[test]
fn snapshot_of_version_with_unclosed_mark_imports_and_reads() {
    let doc = source();
    let expected = checkout_delta(&doc, &mid_mark());
    let checked_out = doc.fork();
    checked_out.checkout(&mid_mark()).unwrap();
    let bytes = checked_out.fork().export(ExportMode::Snapshot).unwrap();

    let other = LoroDoc::new();
    other.import(&bytes).unwrap();
    assert_eq!(
        other.get_text("t").get_richtext_value().to_json_value(),
        expected
    );

    // Re-exporting (including a shallow snapshot, which redacts dead styles) works.
    let again = LoroDoc::new();
    again
        .import(&other.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_eq!(
        again.get_text("t").get_richtext_value().to_json_value(),
        expected
    );
    let shallow = LoroDoc::new();
    shallow
        .import(
            &other
                .export(ExportMode::shallow_snapshot(&other.oplog_frontiers()))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        shallow.get_text("t").get_richtext_value().to_json_value(),
        expected
    );
}

/// A shallow snapshot of a doc whose history ends at the `StyleStart` used to fail
/// with `FrontiersNotFound`: the root was moved to the missing `StyleEnd`.
#[test]
fn shallow_snapshot_of_doc_ending_at_style_start() {
    let doc = source();
    let expected = checkout_delta(&doc, &mid_mark());
    let fork = doc.fork_at(&mid_mark()).unwrap();
    let shallow = LoroDoc::new();
    shallow
        .import(
            &fork
                .export(ExportMode::shallow_snapshot(&fork.oplog_frontiers()))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        shallow.get_text("t").get_richtext_value().to_json_value(),
        expected
    );

    // The rest of the history closes the mark on top of the shallow root.
    shallow
        .import(
            &doc.export(ExportMode::updates(&shallow.oplog_vv()))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        shallow.get_text("t").get_richtext_value().to_json_value(),
        doc.get_text("t").get_richtext_value().to_json_value()
    );
}
