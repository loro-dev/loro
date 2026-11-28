//! Undo/redo of text styles next to a collaborator's concurrent edits
//! (loro-dev/loro#1229, and the undo half of loro-dev/loro#1009).

use loro::{ExpandType, ExportMode, LoroDoc, StyleConfig, StyleConfigMap, ToJson, UndoManager};
use serde_json::json;

fn styled_doc(peer: u64, key: &str, expand: ExpandType) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer).unwrap();
    let mut styles = StyleConfigMap::new();
    styles.insert(key.into(), StyleConfig::new().expand(expand));
    doc.config_text_style(styles);
    doc
}

fn sync(from: &LoroDoc, to: &LoroDoc) {
    to.import(&from.export(ExportMode::updates(&to.oplog_vv())).unwrap())
        .unwrap();
}

fn delta(doc: &LoroDoc) -> serde_json::Value {
    doc.get_text("text").get_richtext_value().to_json_value()
}

/// #1229 case 1: A marks red, B overwrites with blue, A undoes its red.
#[test]
fn undo_keeps_concurrent_remote_style_value() {
    let a = styled_doc(1, "color", ExpandType::After);
    let b = styled_doc(2, "color", ExpandType::After);
    a.get_text("text").insert(0, "x").unwrap();
    a.commit();
    sync(&a, &b);
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);

    a.get_text("text").mark(0..1, "color", "red").unwrap();
    a.commit();
    sync(&a, &b);
    b.get_text("text").mark(0..1, "color", "blue").unwrap();
    b.commit();
    sync(&b, &a);

    let expected = json!([{ "insert": "x", "attributes": { "color": "blue" } }]);
    assert_eq!(delta(&a), expected);
    // The inverse is a no-op: B's blue wins over A's red and over the undo.
    undo.undo().unwrap();
    assert_eq!(delta(&a), expected);
    undo.redo().unwrap();
    assert_eq!(delta(&a), expected);
    sync(&a, &b);
    assert_eq!(delta(&b), expected);
}

/// The undo only drops the keys the remote set, and only on the remote's range.
#[test]
fn undo_drops_only_overlapping_keys_and_ranges() {
    let a = styled_doc(1, "color", ExpandType::After);
    let mut styles = StyleConfigMap::new();
    styles.insert("color".into(), StyleConfig::new().expand(ExpandType::After));
    styles.insert("bold".into(), StyleConfig::new().expand(ExpandType::After));
    a.config_text_style(styles.clone());
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    b.config_text_style(styles);
    a.get_text("text").insert(0, "abcd").unwrap();
    a.commit();
    sync(&a, &b);
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);

    a.get_text("text").mark(0..4, "color", "red").unwrap();
    a.get_text("text").mark(0..4, "bold", true).unwrap();
    a.commit();
    sync(&a, &b);
    b.get_text("text").mark(1..3, "color", "blue").unwrap();
    b.commit();
    sync(&b, &a);

    assert!(undo.undo().unwrap());
    assert_eq!(
        delta(&a),
        json!([
            { "insert": "a" },
            { "insert": "bc", "attributes": { "color": "blue" } },
            { "insert": "d" },
        ])
    );
}

/// #1229 case 2: B removes A's red while A undoes it; A's redo must not bring red back.
#[test]
fn redo_keeps_concurrent_remote_style_removal() {
    let a = styled_doc(1, "color", ExpandType::After);
    let b = styled_doc(2, "color", ExpandType::After);
    a.get_text("text").insert(0, "x").unwrap();
    a.commit();
    sync(&a, &b);
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);

    a.get_text("text").mark(0..1, "color", "red").unwrap();
    a.commit();
    sync(&a, &b);
    b.get_text("text").unmark(0..1, "color").unwrap();
    b.commit();

    assert!(undo.undo().unwrap());
    let plain = json!([{ "insert": "x" }]);
    assert_eq!(delta(&a), plain);
    sync(&b, &a);
    assert_eq!(delta(&a), plain);
    undo.redo().unwrap();
    assert_eq!(delta(&a), plain);
    sync(&a, &b);
    assert_eq!(delta(&b), plain);
}

/// Without concurrent style edits undo/redo of a mark still round-trips.
#[test]
fn undo_redo_mark_without_conflict() {
    let a = styled_doc(1, "color", ExpandType::After);
    let b = styled_doc(2, "color", ExpandType::After);
    a.get_text("text").insert(0, "xy").unwrap();
    a.commit();
    sync(&a, &b);
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    a.get_text("text").mark(0..2, "color", "red").unwrap();
    a.commit();
    b.get_text("text").insert(2, "z").unwrap();
    b.commit();
    sync(&b, &a);
    let red = json!([
        { "insert": "xy", "attributes": { "color": "red" } },
        { "insert": "z" },
    ]);
    assert_eq!(delta(&a), red);
    assert!(undo.undo().unwrap());
    assert_eq!(delta(&a), json!([{ "insert": "xyz" }]));
    assert!(undo.redo().unwrap());
    assert_eq!(delta(&a), red);
}

fn bold_after_concurrent_insert_at_start(peer_a: u64, peer_b: u64) {
    let a = styled_doc(peer_a, "bold", ExpandType::After);
    let b = styled_doc(peer_b, "bold", ExpandType::After);
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    a.get_text("text").insert(0, "important note").unwrap();
    a.commit();
    sync(&a, &b);

    a.get_text("text").mark(0..9, "bold", true).unwrap();
    a.commit();
    b.get_text("text").insert(0, "very ").unwrap();
    b.commit();
    sync(&b, &a);
    // Whether "very " takes the bold is decided by peer order (by design, #1009).
    let merged = delta(&a);
    assert!(
        merged
            == json!([
                { "insert": "very important", "attributes": { "bold": true } },
                { "insert": " note" },
            ])
            || merged
                == json!([
                    { "insert": "very " },
                    { "insert": "important", "attributes": { "bold": true } },
                    { "insert": " note" },
                ]),
        "{merged}"
    );

    assert!(undo.undo().unwrap());
    let plain = json!([{ "insert": "very important note" }]);
    assert_eq!(delta(&a), plain, "peers {peer_a} {peer_b}");
    assert!(undo.redo().unwrap());
    assert_eq!(delta(&a), merged, "peers {peer_a} {peer_b}");
    assert!(undo.undo().unwrap());
    assert_eq!(delta(&a), plain, "peers {peer_a} {peer_b}");
    sync(&a, &b);
    assert_eq!(delta(&b), plain);
}

/// #1009: undoing a mark that absorbed a concurrent insert leaves no residue.
#[test]
fn undo_mark_removes_style_from_absorbed_concurrent_insert() {
    bold_after_concurrent_insert_at_start(1, 2);
    bold_after_concurrent_insert_at_start(2, 1);
}

/// The remote's own style next to the undone range is not the undone style:
/// undo must leave it alone.
#[test]
fn undo_mark_keeps_adjacent_remote_style() {
    let a = styled_doc(1, "bold", ExpandType::After);
    let b = styled_doc(2, "bold", ExpandType::After);
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    a.get_text("text").insert(0, "xximportant").unwrap();
    a.commit();
    sync(&a, &b);
    b.get_text("text").mark(0..2, "bold", true).unwrap();
    b.commit();
    sync(&b, &a);

    a.get_text("text").mark(2..11, "bold", true).unwrap();
    a.commit();
    // B types at the end of its own bold word (expand after keeps it bold).
    b.get_text("text").insert(2, "y").unwrap();
    b.commit();
    sync(&b, &a);
    assert!(undo.undo().unwrap());
    assert_eq!(
        delta(&a),
        json!([
            { "insert": "xxy", "attributes": { "bold": true } },
            { "insert": "important" },
        ])
    );
}

/// Text a collaborator typed inside the marked range (after seeing the mark) took
/// the style; undoing the mark removes it there too, with astral characters
/// around so event lengths (Unicode or UTF-16) matter.
#[test]
fn undo_mark_removes_style_from_remote_text_typed_inside() {
    let a = styled_doc(1, "bold", ExpandType::After);
    let b = styled_doc(2, "bold", ExpandType::After);
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    a.get_text("text").insert(0, "😀ab😀cd😀").unwrap();
    a.commit();
    sync(&a, &b);
    a.get_text("text").mark(1..6, "bold", true).unwrap();
    a.commit();
    sync(&a, &b);
    b.get_text("text").insert(3, "🎉x").unwrap();
    b.get_text("text").mark(0..1, "bold", true).unwrap();
    b.commit();
    sync(&b, &a);
    assert_eq!(
        delta(&a),
        json!([{ "insert": "😀ab🎉x😀cd", "attributes": { "bold": true } }, { "insert": "😀" }])
    );

    assert!(undo.undo().unwrap());
    // B's own bold on the first emoji stays.
    let undone = json!([
        { "insert": "😀", "attributes": { "bold": true } },
        { "insert": "ab🎉x😀cd😀" },
    ]);
    assert_eq!(delta(&a), undone);
    assert!(undo.redo().unwrap());
    assert_eq!(
        delta(&a),
        json!([{ "insert": "😀ab🎉x😀cd", "attributes": { "bold": true } }, { "insert": "😀" }])
    );
    assert!(undo.undo().unwrap());
    assert_eq!(delta(&a), undone);
    sync(&a, &b);
    assert_eq!(delta(&b), undone);
}

/// Only text that took the style is left: a collaborator typed inside the marked
/// range and deleted everything A marked. The positional undo has no styled
/// retain left, so the texts to fix come from the undone style ops.
fn undo_mark_when_only_inherited_text_remains(original: &str, inserted: &str) {
    let a = styled_doc(1, "bold", ExpandType::After);
    let b = styled_doc(2, "bold", ExpandType::After);
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    a.get_text("text").insert(0, original).unwrap();
    a.commit();
    sync(&a, &b);
    let len = a.get_text("text").len_unicode();
    a.get_text("text").mark(0..len, "bold", true).unwrap();
    a.commit();
    sync(&a, &b);
    let text_b = b.get_text("text");
    let first = original.chars().next().unwrap().to_string();
    text_b.insert(first.chars().count(), inserted).unwrap();
    text_b.delete(0, first.chars().count()).unwrap();
    let rest = len - first.chars().count();
    text_b.delete(inserted.chars().count(), rest).unwrap();
    b.commit();
    sync(&b, &a);
    assert_eq!(
        delta(&a),
        json!([{ "insert": inserted, "attributes": { "bold": true } }])
    );

    assert!(undo.undo().unwrap());
    let plain = json!([{ "insert": inserted }]);
    assert_eq!(delta(&a), plain);
    assert!(undo.redo().unwrap());
    assert_eq!(
        delta(&a),
        json!([{ "insert": inserted, "attributes": { "bold": true } }])
    );
    assert!(undo.undo().unwrap());
    assert_eq!(delta(&a), plain);
    sync(&a, &b);
    assert_eq!(delta(&b), plain);
}

#[test]
fn undo_mark_removes_style_when_only_inherited_text_remains() {
    undo_mark_when_only_inherited_text_remains("ab", "x");
    undo_mark_when_only_inherited_text_remains("😀a😀", "🎉x🎉");
}

/// Undo of plain typing reads no history for styles: a step served by its
/// recorded inverse has no style ops.
#[test]
fn typing_undo_next_to_styles_is_unchanged() {
    let a = styled_doc(1, "bold", ExpandType::After);
    a.get_text("text").insert(0, "abc").unwrap();
    a.get_text("text").mark(0..3, "bold", true).unwrap();
    a.commit();
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    a.get_text("text").insert(3, "de").unwrap();
    a.commit();
    assert_eq!(
        delta(&a),
        json!([{ "insert": "abcde", "attributes": { "bold": true } }])
    );
    assert!(undo.undo().unwrap());
    assert_eq!(
        delta(&a),
        json!([{ "insert": "abc", "attributes": { "bold": true } }])
    );
    assert!(undo.redo().unwrap());
    assert_eq!(
        delta(&a),
        json!([{ "insert": "abcde", "attributes": { "bold": true } }])
    );
}
