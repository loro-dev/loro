//! All the tests in this file are based on [richtext.md]

use std::ops::Range;

use loro_common::LoroValue;
use loro_internal::{loro::ExportMode, LoroDoc, ToJson};
use serde_json::json;

fn init(s: &str) -> LoroDoc {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let richtext = doc.get_text("r");
    richtext.insert(0, s).unwrap();
    doc
}

fn clone(doc: &LoroDoc, peer_id: u64) -> LoroDoc {
    let doc2 = LoroDoc::new_auto_commit();
    doc2.set_peer_id(peer_id).unwrap();
    doc2.import(&doc.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    doc2
}

#[derive(Debug)]
enum Kind {
    Bold,
    Italic,
    Link,
}

impl Kind {
    fn key(&self) -> &str {
        match self {
            Kind::Bold => "bold",
            Kind::Link => "link",
            Kind::Italic => "italic",
        }
    }
}

fn insert(doc: &LoroDoc, pos: usize, s: &str) {
    let richtext = doc.get_text("r");
    richtext.insert(pos, s).unwrap();
}

fn delete(doc: &LoroDoc, pos: usize, len: usize) {
    let richtext = doc.get_text("r");
    richtext.delete(pos, len).unwrap();
}

fn mark(doc: &LoroDoc, range: Range<usize>, kind: Kind) {
    let richtext = doc.get_text("r");
    richtext
        .mark(range.start, range.end, kind.key(), true.into())
        .unwrap();
}

fn unmark(doc: &LoroDoc, range: Range<usize>, kind: Kind) {
    let richtext = doc.get_text("r");
    richtext
        .mark(range.start, range.end, kind.key(), false.into())
        .unwrap();
}

fn mark_kv(doc: &LoroDoc, range: Range<usize>, key: &str, value: impl Into<LoroValue>) {
    let richtext = doc.get_text("r");
    richtext
        .mark(range.start, range.end, key, value.into())
        .unwrap();
}

fn merge(a: &LoroDoc, b: &LoroDoc) {
    a.import(&b.export(ExportMode::updates(&a.oplog_vv())).unwrap())
        .unwrap();
    b.import(&a.export(ExportMode::updates(&b.oplog_vv())).unwrap())
        .unwrap();
}

fn expect_result(doc: &LoroDoc, json: serde_json::Value) {
    let richtext = doc.get_text("r");
    let s = richtext.get_richtext_value().to_json_value();
    assert_eq!(
        &s,
        &json,
        "expect: {}, got: {}",
        serde_json::to_string_pretty(&json).unwrap(),
        serde_json::to_string_pretty(&s).unwrap()
    );
}

#[test]
fn case0() {
    let doc_a = init("Hello World");
    let doc_b = clone(&doc_a, 2);
    mark(&doc_a, 0..11, Kind::Bold);
    insert(&doc_b, 6, "New ");
    merge(&doc_a, &doc_b);
    expect_result(
        &doc_a,
        json!([{"insert":"Hello New World","attributes":{"bold":true}}]),
    );
    doc_a.check_state_diff_calc_consistency_slow();
    doc_b.check_state_diff_calc_consistency_slow();
}

#[test]
fn case1() {
    let doc_a = init("Hello World");
    let doc_b = clone(&doc_a, 2);
    mark(&doc_a, 0..5, Kind::Bold);
    mark(&doc_b, 3..11, Kind::Bold);
    merge(&doc_a, &doc_b);
    expect_result(
        &doc_a,
        json!([{"insert":"Hello World","attributes":{"bold":true}}]),
    );
    doc_a.check_state_diff_calc_consistency_slow();
    doc_b.check_state_diff_calc_consistency_slow();
}

#[test]
fn case2() {
    let doc_a = init("Hello World");
    mark(&doc_a, 0..11, Kind::Bold);
    let doc_b = clone(&doc_a, 2);
    unmark(&doc_a, 0..6, Kind::Bold);
    insert(&doc_b, 5, " a");
    merge(&doc_a, &doc_b);
    expect_result(
        &doc_a,
        json!([{"insert":"Hello a ","attributes":{"bold":false}},{"insert":"World","attributes":{"bold":true}}]),
    );
    doc_a.check_state_diff_calc_consistency_slow();
    doc_b.check_state_diff_calc_consistency_slow();
}

/// | Name            | Text                   |
/// |:----------------|:-----------------------|
/// | Origin          | `Hello World`          |
/// | Concurrent A    | `Hello <b>World</b>`   |
/// | Concurrent B    | `Hello a World`        |
/// | Expected Result | `Hello a <b>World</b>` |
///
#[test]
fn case3() {
    let doc_a = init("Hello World");
    let doc_b = clone(&doc_a, 2);
    mark(&doc_a, 6..11, Kind::Bold);
    insert(&doc_b, 5, " a");
    merge(&doc_a, &doc_b);
    expect_result(
        &doc_a,
        json!([{"insert":"Hello a "},{"insert":"World","attributes":{"bold":true}}]),
    );
    doc_a.check_state_diff_calc_consistency_slow();
    doc_b.check_state_diff_calc_consistency_slow();
}

/// | Name            | Text                       |
/// |:----------------|:---------------------------|
/// | Origin          | `Hello World`              |
/// | Concurrent A    | `<link>Hello</link> World` |
/// | Concurrent B    | `Hey World`                |
/// | Expected Result | `<link>Hey</link> World`   |
#[test]
fn case4() {
    let doc_a = init("Hello World");
    let doc_b = clone(&doc_a, 2);
    mark(&doc_a, 0..5, Kind::Link);
    delete(&doc_b, 2, 3);
    expect_result(&doc_b, json!([{"insert":"He World"}]));
    insert(&doc_b, 2, "y");
    expect_result(&doc_b, json!([{"insert":"Hey World"}]));
    merge(&doc_a, &doc_b);
    expect_result(
        &doc_b,
        json!([{"insert":"Hey","attributes":{"link":true}},{"insert":" World"}]),
    );
    expect_result(
        &doc_a,
        json!([{"insert":"Hey","attributes":{"link":true}},{"insert":" World"}]),
    );
    doc_a.check_state_diff_calc_consistency_slow();
    doc_b.check_state_diff_calc_consistency_slow();
}

/// When insert a new character after "Hello", the new char should be bold but not link
///
/// | Name            | Text                              |
/// |:----------------|:----------------------------------|
/// | Origin          | `<b><link>Hello</link><b> World`  |
/// | Expected Result | `<b><link>Hello</link>t<b> World` |
#[test]
fn case5() {
    let doc = init("Hello World");
    mark(&doc, 0..5, Kind::Bold);
    expect_result(
        &doc,
        serde_json::json!([
            {"insert": "Hello", "attributes": {"bold": true}},
            {"insert": " World"}
        ]),
    );
    mark(&doc, 0..5, Kind::Link);
    expect_result(
        &doc,
        serde_json::json!([
            {"insert": "Hello", "attributes": {"bold": true, "link": true}},
            {"insert": " World"}
        ]),
    );
    insert(&doc, 5, "t");
    expect_result(
        &doc,
        serde_json::json!([
            {"insert": "Hello", "attributes": {"bold": true, "link": true}},
            {"insert": "t", "attributes": {"bold": true}},
            {"insert": " World"}
        ]),
    );
    doc.check_state_diff_calc_consistency_slow();
}

///
/// | Name            | Text                                         |
/// |:----------------|:---------------------------------------------|
/// | Origin          | `<b>The fox jumped</b> over the dog.`        |
/// | Concurrent A    | `The fox jumped over the dog.`               |
/// | Concurrent B    | `<b>The </b>fox<b> jumped</b> over the dog.` |
/// | Expected Result | `The fox jumped over the dog.`               |
#[test]
fn case6() {
    let doc_a = init("The fox jumped over the dog.");
    mark(&doc_a, 0..3, Kind::Bold);
    let doc_b = clone(&doc_a, 2);
    unmark(&doc_a, 0..3, Kind::Bold);
    unmark(&doc_b, 4..7, Kind::Bold);
    merge(&doc_a, &doc_b);
    expect_result(
        &doc_a,
        json!([
            {"insert":"The", "attributes": {"bold": false}},
            {"insert":" ",},
            {"insert":"fox", "attributes": {"bold": false}},
            {"insert":" jumped over the dog."}
        ]),
    );
    doc_a.check_state_diff_calc_consistency_slow();
    doc_b.check_state_diff_calc_consistency_slow();
}

/// | Name            | Text                                         |
/// |:----------------|:---------------------------------------------|
/// | Origin          | `<b>The fox jumped</b> over the dog.`        |
/// | Concurrent A    | `<b>The fox</b> jumped over the dog.`        |
/// | Concurrent B    | `<b>The</b> fox jumped over the <b>dog</b>.` |
/// | Expected Result | `<b>The</b> fox jumped over the <b>dog</b>.` |
#[test]
fn case7() {
    let doc_a = init("The fox jumped over the dog.");
    mark(&doc_a, 0..14, Kind::Bold);
    let doc_b = clone(&doc_a, 2);
    unmark(&doc_a, 7..14, Kind::Bold);
    unmark(&doc_b, 3..14, Kind::Bold);
    mark(&doc_b, 24..27, Kind::Bold);
    merge(&doc_a, &doc_b);
    expect_result(
        &doc_a,
        serde_json::json!([
            {"insert": "The", "attributes": {"bold": true}},
            {"insert": " fox jumped", "attributes": {"bold": false}},
            {"insert": " over the "},
            {"insert": "dog", "attributes": {"bold": true}},
            {"insert": "."}
        ]),
    );
    doc_a.check_state_diff_calc_consistency_slow();
    doc_b.check_state_diff_calc_consistency_slow();
}

/// | Name            | Text                         |
/// |:----------------|:-----------------------------|
/// | Origin          | The fox jumped.              |
/// | Concurrent A    | **The fox** jumped.          |
/// | Concurrent B    | The *fox jumped*.            |
/// | Expected Result | **The _fox_**<i> jumped</i>. |
#[test]
fn case8() {
    let doc_a = init("The fox jumped.");
    let doc_b = clone(&doc_a, 2);
    mark(&doc_a, 0..7, Kind::Bold);
    mark(&doc_a, 4..14, Kind::Italic);
    merge(&doc_a, &doc_b);
    expect_result(
        &doc_a,
        serde_json::json!([
            {"insert": "The ", "attributes": {"bold": true}},
            {"insert": "fox", "attributes": {"bold": true, "italic": true}},
            {"insert": " jumped", "attributes": {"italic": true}},
            {"insert": "."}
        ]),
    );
    doc_a.check_state_diff_calc_consistency_slow();
    doc_b.check_state_diff_calc_consistency_slow();
}

/// ![](https://i.postimg.cc/MTNGq8cH/Clean-Shot-2023-10-09-at-12-16-29-2x.png)
#[test]
fn case9() {
    let doc_a = init("The fox jumped.");
    let doc_b = clone(&doc_a, 2);
    mark_kv(&doc_a, 0..7, "comment:alice", "alice comment");
    mark_kv(&doc_a, 4..14, "comment:bob", "bob comment");
    merge(&doc_a, &doc_b);
    expect_result(
        &doc_a,
        serde_json::json!([
            {"insert": "The ", "attributes": {"comment:alice": "alice comment"}},
            {"insert": "fox", "attributes": {"comment:alice": "alice comment", "comment:bob": "bob comment"}},
            {"insert": " jumped", "attributes": {"comment:bob": "bob comment"}},
            {"insert": "."}
        ]),
    );
}

#[test]
fn insert_after_link() {
    let doc_a = init("The fox jumped.");
    let doc_b = clone(&doc_a, 2);
    mark(&doc_a, 0..3, Kind::Link);
    merge(&doc_a, &doc_b);
    insert(&doc_a, 3, "a");
    merge(&doc_a, &doc_b);
    expect_result(
        &doc_a,
        serde_json::json!([
            {"insert": "The", "attributes": {"link": true}},
            {"insert": "a fox jumped."},
        ]),
    );
    expect_result(
        &doc_b,
        serde_json::json!([
            {"insert": "The", "attributes": {"link": true}},
            {"insert": "a fox jumped."},
        ]),
    );
    doc_a.check_state_diff_calc_consistency_slow();
    doc_b.check_state_diff_calc_consistency_slow();
}
