//! Forged Text ops whose positions lie past the end of the text used to panic
//! inside the doc locks (loro-dev/loro#1160). They are now rejected with `Err`
//! and leave the document untouched and usable, like out-of-bounds List and
//! MovableList ops.

use loro::{ExportMode, LoroDoc, LoroValue, VersionVector};
use serde_json::{json, Value};

/// Peer 1 writes `"ab"` (counters/lamports 0..=1).
fn base() -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.get_text("t").insert(0, "ab").unwrap();
    doc.commit();
    doc
}

/// `base()` plus a concurrent edit by peer 2, so the forged op is not a plain
/// fast-forward of the state.
fn base_with_concurrent_edit() -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(2).unwrap();
    doc.get_text("t").insert(0, "xy").unwrap();
    doc.commit();
    doc.import(&base().export(ExportMode::all_updates()).unwrap())
        .unwrap();
    doc
}

/// A single change by peer 1, causally after `base()`, holding `ops`.
fn forged_ops(ops: &[Value]) -> String {
    let mut counter = 2;
    let ops: Vec<Value> = ops
        .iter()
        .map(|content| {
            let op =
                json!({"container": "cid:root-t:Text", "content": content, "counter": counter});
            counter += match content["type"].as_str().unwrap() {
                "insert" => content["text"].as_str().unwrap().chars().count() as i64,
                "delete" => content["len"].as_i64().unwrap().abs(),
                _ => 1,
            };
            op
        })
        .collect();
    json!({
        "schema_version": 1,
        "start_version": {},
        "peers": ["1"],
        "changes": [{
            "id": "2@0",
            "timestamp": 0,
            "deps": ["1@0"],
            "lamport": 2,
            "msg": null,
            "ops": ops
        }]
    })
    .to_string()
}

fn forged(op: Value) -> String {
    forged_ops(&[op])
}

fn mark(start: u32, end: u32) -> Vec<Value> {
    vec![
        json!({"type": "mark", "start": start, "end": end, "style_key": "bold", "style_value": true, "info": 0}),
        json!({"type": "mark_end"}),
    ]
}

struct Snapshot {
    value: LoroValue,
    richtext: LoroValue,
    vv: VersionVector,
}

impl Snapshot {
    fn of(doc: &LoroDoc) -> Self {
        Self {
            value: doc.get_deep_value(),
            richtext: doc.get_text("t").get_richtext_value(),
            vv: doc.oplog_vv(),
        }
    }

    fn assert_unchanged(&self, doc: &LoroDoc) {
        assert_eq!(doc.get_deep_value(), self.value);
        assert_eq!(doc.get_text("t").get_richtext_value(), self.richtext);
        assert_eq!(doc.oplog_vv(), self.vv);
    }
}

/// The doc still accepts local edits and valid remote updates after a rejected import.
fn assert_usable(doc: &LoroDoc) {
    let text = doc.get_text("t");
    let len = text.len_unicode();
    text.insert(len, "!").unwrap();
    text.mark(0..1, "italic", true).unwrap();
    text.delete(0, 1).unwrap();
    doc.commit();
    assert_eq!(text.len_unicode(), len);

    let remote = LoroDoc::new();
    remote
        .import(&doc.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    remote.get_text("t").insert(0, "r").unwrap();
    remote.commit();
    doc.import(&remote.export(ExportMode::updates(&doc.oplog_vv())).unwrap())
        .unwrap();
    assert_eq!(doc.get_deep_value(), remote.get_deep_value());
}

fn assert_rejected(doc: &LoroDoc, json: String) {
    let before = Snapshot::of(doc);
    let err = doc.import_json_updates(json).unwrap_err();
    assert!(
        matches!(err, loro::LoroError::DecodeError(_)),
        "unexpected error: {err:?}"
    );
    before.assert_unchanged(doc);
    assert_usable(doc);
}

/// Detached imports only touch the oplog, which lets us export a forged op as binary.
fn forged_binary(json: String) -> Vec<u8> {
    let carrier = base();
    carrier.detach();
    carrier.import_json_updates(json).unwrap();
    carrier
        .export(ExportMode::updates(&base().oplog_vv()))
        .unwrap()
}

fn out_of_bounds_ops() -> Vec<Vec<Value>> {
    vec![
        // loro-dev/loro#1160
        vec![json!({"type": "insert", "pos": 3, "text": "x"})],
        vec![json!({"type": "insert", "pos": 100, "text": "x"})],
        vec![json!({"type": "delete", "pos": 1, "len": 2, "start_id": "1@0"})],
        vec![json!({"type": "delete", "pos": 2, "len": 1, "start_id": "1@0"})],
        // A reversed delete removes `pos + len + 1..=pos`.
        vec![json!({"type": "delete", "pos": 2, "len": -2, "start_id": "1@0"})],
        vec![json!({"type": "delete", "pos": 5, "len": -2, "start_id": "1@0"})],
        mark(0, 3),
        mark(3, 4),
        mark(1, 9),
        // Out of bounds only after an earlier op of the same change.
        vec![
            json!({"type": "delete", "pos": 0, "len": 2, "start_id": "0@0"}),
            json!({"type": "insert", "pos": 1, "text": "x"}),
        ],
    ]
}

#[test]
fn text_insert_past_end_is_rejected() {
    // The exact repro from loro-dev/loro#1160.
    assert_rejected(
        &base(),
        forged(json!({"type": "insert", "pos": 3, "text": "x"})),
    );
}

#[test]
fn out_of_bounds_text_ops_are_rejected() {
    for ops in out_of_bounds_ops() {
        assert_rejected(&base(), forged_ops(&ops));
        assert_rejected(&base_with_concurrent_edit(), forged_ops(&ops));
    }
}

#[test]
fn out_of_bounds_text_ops_are_rejected_in_binary_imports() {
    for ops in out_of_bounds_ops() {
        let update = forged_binary(forged_ops(&ops));

        let doc = base();
        let before = Snapshot::of(&doc);
        doc.import(&update).unwrap_err();
        before.assert_unchanged(&doc);
        assert_usable(&doc);

        let doc = base();
        let before = Snapshot::of(&doc);
        doc.import_batch(&[update.clone()]).unwrap_err();
        assert!(!doc.is_detached());
        before.assert_unchanged(&doc);
        assert_usable(&doc);

        let doc = base_with_concurrent_edit();
        let before = Snapshot::of(&doc);
        doc.import(&update).unwrap_err();
        before.assert_unchanged(&doc);
        assert_usable(&doc);
    }
}

#[test]
fn in_bounds_text_ops_still_import() {
    let cases = [
        (
            vec![json!({"type": "insert", "pos": 2, "text": "x"})],
            "abx",
        ),
        (
            vec![json!({"type": "delete", "pos": 0, "len": 2, "start_id": "0@0"})],
            "",
        ),
        (
            vec![json!({"type": "delete", "pos": 1, "len": -2, "start_id": "1@0"})],
            "",
        ),
        (mark(0, 2), "ab"),
        (mark(1, 2), "ab"),
    ];
    for (ops, expected) in cases {
        let doc = base();
        doc.import_json_updates(forged_ops(&ops)).unwrap();
        assert_eq!(doc.get_text("t").to_string(), expected);
        assert_usable(&doc);

        let doc = base_with_concurrent_edit();
        doc.import_json_updates(forged_ops(&ops)).unwrap();
        assert_usable(&doc);
    }
}

#[test]
fn out_of_bounds_text_op_after_map_change_in_same_import_is_rejected() {
    // The forged change depends on a map-only change of the same import, so its
    // deps are not in the DAG when the import starts.
    let json = json!({
        "schema_version": 1, "start_version": {}, "peers": ["1"],
        "changes": [
            {"id": "2@0", "timestamp": 0, "deps": ["1@0"], "lamport": 2, "msg": null,
             "ops": [{"container": "cid:root-meta:Map",
                      "content": {"type": "insert", "key": "k", "value": 1}, "counter": 2}]},
            {"id": "3@0", "timestamp": 0, "deps": ["2@0"], "lamport": 3, "msg": null,
             "ops": [{"container": "cid:root-t:Text",
                      "content": {"type": "insert", "pos": 3, "text": "x"}, "counter": 3}]}
        ]
    })
    .to_string();
    assert_rejected(&base(), json);
}

#[test]
fn map_only_import_that_unlocks_parked_out_of_bounds_text_op_is_rejected() {
    let change = |id: &str, dep: &str, lamport: u32, op: Value| {
        json!({
            "schema_version": 1, "start_version": {}, "peers": ["1"],
            "changes": [{"id": id, "timestamp": 0, "deps": [dep], "lamport": lamport,
                         "msg": null, "ops": [op]}]
        })
        .to_string()
    };
    let map_change = change(
        "2@0",
        "1@0",
        2,
        json!({"container": "cid:root-meta:Map",
               "content": {"type": "insert", "key": "k", "value": 1}, "counter": 2}),
    );
    let forged_text = change(
        "3@0",
        "2@0",
        3,
        json!({"container": "cid:root-t:Text",
               "content": {"type": "insert", "pos": 3, "text": "x"}, "counter": 3}),
    );

    let doc = base();
    // Its dep is missing, so the forged change is parked, not validated yet.
    let status = doc.import_json_updates(forged_text).unwrap();
    assert!(status.pending.is_some());
    // The map-only import unlocks it, so it must still get a rollback scope.
    assert_rejected(&doc, map_change);
}
