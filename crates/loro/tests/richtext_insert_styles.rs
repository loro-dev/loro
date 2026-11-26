//! A local text insert takes, for each style key, what its displayed
//! neighbours and their expand settings imply, whatever style anchors lie
//! between them (loro-dev/loro#1240). See
//! `context/richtext-insert-positions.md`.

use loro::{ExpandType, ExportMode, LoroDoc, LoroResult, StyleConfig, StyleConfigMap, ToJson};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

fn doc_with(peer: u64, styles: &[(&str, ExpandType)]) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer).unwrap();
    let mut config = StyleConfigMap::new();
    for (key, expand) in styles {
        config.insert((*key).into(), StyleConfig { expand: *expand });
    }
    doc.config_text_style(config);
    doc
}

fn delta(doc: &LoroDoc) -> Value {
    doc.get_text("t").get_richtext_value().to_json_value()
}

/// Shape 1: a highlight (expand both) is removed; text typed where it began
/// must not be highlighted.
#[test]
fn insert_after_unmark_of_both_expanding_style_is_plain() -> LoroResult<()> {
    let doc = doc_with(1, &[("hl", ExpandType::Both)]);
    let t = doc.get_text("t");
    t.insert(0, "ab")?;
    doc.commit();
    t.mark(0..2, "hl", true)?;
    doc.commit();
    t.unmark(0..2, "hl")?;
    doc.commit();
    t.insert(0, "X")?;
    doc.commit();
    assert_eq!(delta(&doc), json!([{ "insert": "Xab" }]));
    t.insert(3, "Y")?;
    assert_eq!(delta(&doc), json!([{ "insert": "XabY" }]));
    Ok(())
}

/// Shape 1, deleted variant: the only highlighted character is deleted, and a
/// character typed in its place takes no highlight.
#[test]
fn insert_where_only_styled_char_was_deleted_is_plain() -> LoroResult<()> {
    for expand in [
        ExpandType::Both,
        ExpandType::None,
        ExpandType::After,
        ExpandType::Before,
    ] {
        let doc = doc_with(1, &[("hl", expand)]);
        let t = doc.get_text("t");
        t.insert(0, "abc")?;
        t.mark(1..2, "hl", true)?;
        doc.commit();
        t.delete(1, 1)?;
        t.insert(1, "X")?;
        doc.commit();
        assert_eq!(delta(&doc), json!([{ "insert": "aXc" }]), "{expand:?}");
    }
    Ok(())
}

/// Shape 2: a link (expand none) with a hole; text typed at the left edge of
/// the hole is not linked. A bold (expand after) in the same shape extends.
#[test]
fn insert_at_left_edge_of_unstyled_hole_follows_expand() -> LoroResult<()> {
    let run = |key: &str, expand| -> LoroResult<Value> {
        let doc = doc_with(1, &[(key, expand)]);
        let t = doc.get_text("t");
        t.insert(0, "abc")?;
        doc.commit();
        t.mark(0..3, key, true)?;
        doc.commit();
        t.unmark(1..2, key)?;
        doc.commit();
        t.insert(1, "X")?;
        doc.commit();
        Ok(delta(&doc))
    };
    assert_eq!(
        run("link", ExpandType::None)?,
        json!([
            { "insert": "a", "attributes": { "link": true } },
            { "insert": "Xb" },
            { "insert": "c", "attributes": { "link": true } },
        ])
    );
    assert_eq!(
        run("bold", ExpandType::After)?,
        json!([
            { "insert": "aX", "attributes": { "bold": true } },
            { "insert": "b" },
            { "insert": "c", "attributes": { "bold": true } },
        ])
    );
    // A before-expanding style at the right edge of the hole extends left.
    let doc = doc_with(1, &[("sup", ExpandType::Before)]);
    let t = doc.get_text("t");
    t.insert(0, "abc")?;
    t.mark(0..3, "sup", true)?;
    t.unmark(1..2, "sup")?;
    t.insert(2, "X")?;
    assert_eq!(
        delta(&doc),
        json!([
            { "insert": "a", "attributes": { "sup": true } },
            { "insert": "b" },
            { "insert": "Xc", "attributes": { "sup": true } },
        ])
    );
    Ok(())
}

/// Shape 3: two adjacent marks with one value display like one mark, and an
/// insert between them is styled in both cases.
#[test]
fn insert_between_split_marks_with_same_value_is_styled() -> LoroResult<()> {
    for split in [true, false] {
        let doc = doc_with(1, &[("link", ExpandType::None)]);
        let t = doc.get_text("t");
        t.insert(0, "ab")?;
        doc.commit();
        if split {
            t.mark(0..1, "link", "u")?;
            t.mark(1..2, "link", "u")?;
        } else {
            t.mark(0..2, "link", "u")?;
        }
        doc.commit();
        assert_eq!(
            delta(&doc),
            json!([{ "insert": "ab", "attributes": { "link": "u" } }])
        );
        t.insert(1, "X")?;
        doc.commit();
        assert_eq!(
            delta(&doc),
            json!([{ "insert": "aXb", "attributes": { "link": "u" } }]),
            "split={split}"
        );
    }

    // Different values on the two sides: a non-expanding style gives neither.
    let doc = doc_with(1, &[("link", ExpandType::None)]);
    let t = doc.get_text("t");
    t.insert(0, "ab")?;
    t.mark(0..1, "link", "u")?;
    t.mark(1..2, "link", "v")?;
    t.insert(1, "X")?;
    assert_eq!(
        delta(&doc),
        json!([
            { "insert": "a", "attributes": { "link": "u" } },
            { "insert": "X" },
            { "insert": "b", "attributes": { "link": "v" } },
        ])
    );
    Ok(())
}

/// Shape 4: concurrent bold (expand after) and sup (expand before) on the
/// same character; text typed before it is sup and not bold, whatever the
/// order of the two start anchors.
#[test]
fn insert_before_concurrent_styles_does_not_depend_on_anchor_order() -> LoroResult<()> {
    let config = [("bold", ExpandType::After), ("sup", ExpandType::Before)];
    let run = |k1: &str, k2: &str| -> LoroResult<(Value, usize)> {
        let d1 = doc_with(1, &config);
        d1.get_text("t").insert(0, "y")?;
        d1.commit();
        let d2 = doc_with(2, &config);
        d2.import(&d1.export(ExportMode::Snapshot)?)?;
        d1.get_text("t").mark(0..1, k1, true)?;
        d1.commit();
        d2.get_text("t").mark(0..1, k2, true)?;
        d2.commit();
        d1.import(&d2.export(ExportMode::updates(&d1.oplog_vv()))?)?;
        let ops_before = d1.len_ops();
        d1.get_text("t").insert(0, "X")?;
        d1.commit();
        let insert_ops = d1.len_ops() - ops_before;

        // Remote peers see the same result.
        let d3 = doc_with(3, &config);
        d3.import(&d1.export(ExportMode::Snapshot)?)?;
        assert_eq!(delta(&d3), delta(&d1));
        d2.import(&d1.export(ExportMode::updates(&d2.oplog_vv()))?)?;
        assert_eq!(delta(&d2), delta(&d1));
        Ok((delta(&d1), insert_ops))
    };
    let expected = json!([
        { "insert": "X", "attributes": { "sup": true } },
        { "insert": "y", "attributes": { "bold": true, "sup": true } },
    ]);
    let (a, a_ops) = run("bold", "sup")?;
    let (b, b_ops) = run("sup", "bold")?;
    assert_eq!(a, expected);
    assert_eq!(b, expected);
    // In one order a position between the start anchors gives exactly `sup`;
    // in the other none does, and the insert also writes a `sup` mark (one
    // text op plus a start and an end anchor).
    let mut ops = [a_ops, b_ops];
    ops.sort();
    assert_eq!(ops, [1, 3]);
    Ok(())
}

/// Typing continues a style that expands after, and typing right after a
/// non-expanding one stays plain; every keystroke rechecks (the insert cursor
/// cache must not skip the rule).
#[test]
fn consecutive_typing_next_to_anchors_follows_the_rule() -> LoroResult<()> {
    let doc = doc_with(
        1,
        &[("bold", ExpandType::After), ("link", ExpandType::None)],
    );
    let t = doc.get_text("t");
    t.insert(0, "ab")?;
    t.mark(0..1, "bold", true)?;
    t.mark(0..1, "link", "u")?;
    for (i, ch) in ["1", "2", "3"].into_iter().enumerate() {
        t.insert(1 + i, ch)?;
    }
    assert_eq!(
        delta(&doc),
        json!([
            { "insert": "a", "attributes": { "bold": true, "link": "u" } },
            { "insert": "123", "attributes": { "bold": true } },
            { "insert": "b" },
        ])
    );
    Ok(())
}

/// Inserts with explicit attributes still get exactly those attributes.
#[test]
fn insert_with_attributes_is_unchanged() -> LoroResult<()> {
    let config = [("bold", ExpandType::After), ("sup", ExpandType::Before)];
    let d1 = doc_with(1, &config);
    d1.get_text("t").insert(0, "y")?;
    d1.commit();
    let d2 = doc_with(2, &config);
    d2.import(&d1.export(ExportMode::Snapshot)?)?;
    d1.get_text("t").mark(0..1, "bold", true)?;
    d2.get_text("t").mark(0..1, "sup", true)?;
    d1.commit();
    d2.commit();
    d1.import(&d2.export(ExportMode::updates(&d1.oplog_vv()))?)?;
    d1.get_text("t").apply_delta(&[loro::TextDelta::Insert {
        insert: "X".into(),
        attributes: Some([("bold".to_string(), true.into())].into_iter().collect()),
    }])?;
    assert_eq!(
        delta(&d1),
        json!([
            { "insert": "X", "attributes": { "bold": true } },
            { "insert": "y", "attributes": { "bold": true, "sup": true } },
        ])
    );
    Ok(())
}
