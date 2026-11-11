//! A text `StyleEnd` op whose previous op is not its `StyleStart` used to panic in
//! the richtext diff calculator (`style_for_end_anchor`) while the doc held its
//! locks, leaving the doc unusable (loro-dev/loro#1254). Such imports are now
//! rejected with `Err` and leave the document untouched and usable.

use base64::Engine;
use loro::{ExportMode, IdSpan, LoroDoc, LoroValue, VersionVector};
use serde_json::{json, Value};

/// Peer 1 inserts "hello" into root text "t" (counters/lamports 0..=4).
fn base() -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.get_text("t").insert(0, "hello").unwrap();
    doc.commit();
    doc
}

/// A single change by peer 1, causally after `base()`, holding `ops` from counter 5.
fn forged(ops: Vec<Value>) -> String {
    let ops: Vec<Value> = ops
        .into_iter()
        .enumerate()
        .map(|(i, mut op)| {
            op["counter"] = json!(5 + i);
            op
        })
        .collect();
    json!({
        "schema_version": 1,
        "start_version": {},
        "peers": ["1"],
        "changes": [{
            "id": "5@0",
            "timestamp": 0,
            "deps": ["4@0"],
            "lamport": 5,
            "msg": null,
            "ops": ops,
        }]
    })
    .to_string()
}

fn mark_end_on(container: &str) -> Value {
    json!({"container": container, "content": {"type": "mark_end"}})
}

fn mark_on(container: &str) -> Value {
    json!({
        "container": container,
        "content": {
            "type": "mark",
            "start": 0,
            "end": 2,
            "style_key": "bold",
            "style_value": true,
            "info": 0,
        }
    })
}

struct Snapshot {
    value: LoroValue,
    vv: VersionVector,
}

impl Snapshot {
    fn of(doc: &LoroDoc) -> Self {
        Self {
            value: doc.get_deep_value(),
            vv: doc.oplog_vv(),
        }
    }

    fn assert_unchanged(&self, doc: &LoroDoc) {
        assert_eq!(doc.get_deep_value(), self.value);
        assert_eq!(doc.oplog_vv(), self.vv);
    }
}

/// The doc still accepts local edits and valid remote updates after a rejected import.
fn assert_usable(doc: &LoroDoc) {
    let text = doc.get_text("t");
    let len = text.len_unicode();
    text.insert(0, "x").unwrap();
    doc.commit();
    assert_eq!(text.len_unicode(), len + 1);

    let remote = LoroDoc::new();
    remote.set_peer_id(9).unwrap();
    remote
        .import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    remote.get_text("t").insert(0, "remote").unwrap();
    remote.commit();
    doc.import(&remote.export(ExportMode::updates(&doc.oplog_vv())).unwrap())
        .unwrap();
    assert_eq!(doc.get_deep_value(), remote.get_deep_value());
}

fn assert_json_rejected(doc: &LoroDoc, json: String) {
    let before = Snapshot::of(doc);
    let err = doc.import_json_updates(json).unwrap_err();
    assert!(
        matches!(err, loro::LoroError::DecodeError(_)),
        "unexpected error: {err:?}"
    );
    before.assert_unchanged(doc);
    if doc.is_detached() {
        doc.attach();
    }
    assert_usable(doc);
}

/// One peer inserts "hello" into root text "t".
const ISSUE_1254_BASE: &str =
    "bG9ybwAAAAAAAAAAAAAAAM2Gk5MABD4ABQAFARABAQAAAAAAAAABAQAAAAAABQEAAAEABgEEAQIAAAIBdAAOAQQCAQACAQACAQUCAQUABgVoZWxsbw==";
/// The next update from that peer (insert "ab" at 0) with byte 74 changed from
/// 0x05 to 0x00 and the checksum recomputed.
const ISSUE_1254_UPDATE: &str =
    "bG9ybwAAAAAAAAAAAAAAAPGPTawABDwFAgUCAREBAQAAAAAAAAAAAQEAAAAAAAUBAAABAAYBBAECAAACAXQADgEEAgEAAgEAAgEAAgECAAMCYWI=";

/// The malformed update from loro-dev/loro#1254, imported on top of its base.
#[test]
fn issue_1254_malformed_update_is_rejected() {
    let engine = base64::engine::general_purpose::STANDARD;
    let base = engine.decode(ISSUE_1254_BASE).unwrap();
    let update = engine.decode(ISSUE_1254_UPDATE).unwrap();

    let doc = LoroDoc::new();
    doc.import(&base).unwrap();
    let before = Snapshot::of(&doc);
    assert!(doc.import(&update).is_err());
    before.assert_unchanged(&doc);
    assert_usable(&doc);
}

#[test]
fn style_end_after_insert_is_rejected() {
    assert_json_rejected(&base(), forged(vec![mark_end_on("cid:root-t:Text")]));
}

#[test]
fn style_end_after_insert_is_rejected_when_detached() {
    let doc = base();
    doc.detach();
    assert_json_rejected(&doc, forged(vec![mark_end_on("cid:root-t:Text")]));
}

#[test]
fn style_end_after_style_start_in_other_text_is_rejected() {
    assert_json_rejected(
        &base(),
        forged(vec![
            mark_on("cid:root-other:Text"),
            mark_end_on("cid:root-t:Text"),
        ]),
    );
}

#[test]
fn style_end_after_map_op_is_rejected() {
    assert_json_rejected(
        &base(),
        forged(vec![
            json!({
                "container": "cid:root-m:Map",
                "content": {"type": "insert", "key": "k", "value": 1}
            }),
            mark_end_on("cid:root-t:Text"),
        ]),
    );
}

#[test]
fn style_end_as_first_op_of_peer_is_rejected() {
    let doc = LoroDoc::new();
    doc.set_peer_id(2).unwrap();
    let json = json!({
        "schema_version": 1,
        "start_version": {},
        "peers": ["1"],
        "changes": [{
            "id": "0@0",
            "timestamp": 0,
            "deps": [],
            "lamport": 0,
            "msg": null,
            "ops": [{
                "container": "cid:root-t:Text",
                "content": {"type": "mark_end"},
                "counter": 0,
            }],
        }]
    })
    .to_string();
    assert_json_rejected(&doc, json);
}

/// An honest mark whose `StyleStart` and `StyleEnd` arrive in different updates is
/// still accepted, attached and detached.
#[test]
fn style_end_split_from_its_start_is_accepted() {
    let src = base();
    src.get_text("t").mark(0..2, "bold", true).unwrap();
    src.commit();
    // Counter 5 is the StyleStart, counter 6 the StyleEnd.
    let start = src
        .export(ExportMode::updates_in_range(vec![IdSpan::new(1, 5, 6)]))
        .unwrap();
    let end = src
        .export(ExportMode::updates_in_range(vec![IdSpan::new(1, 6, 7)]))
        .unwrap();

    for detached in [false, true] {
        let doc = base();
        if detached {
            doc.detach();
        }
        doc.import(&start).unwrap();
        doc.import(&end).unwrap();
        doc.attach();
        assert_eq!(doc.get_deep_value(), src.get_deep_value());
        assert_eq!(
            doc.get_text("t").get_richtext_value(),
            src.get_text("t").get_richtext_value()
        );
    }
}
