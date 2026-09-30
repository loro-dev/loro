//! History relayed through JSON keeps its lossy values even after binary export.
//! Known prefixes must be compared with that loss in mind, then trimmed without
//! replacing the receiver's original values.

use loro::{
    ExpandType, ExportMode, LoroDoc, LoroError, LoroValue, StyleConfig, ValueOrContainer, ID,
};

fn source() -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.config_default_text_style(Some(StyleConfig {
        expand: ExpandType::After,
    }));
    doc
}

fn json_copy(doc: &LoroDoc) -> LoroDoc {
    let json = doc.export_json_updates(&Default::default(), &doc.oplog_vv());
    // A typed JsonSchema does not lose values: exercise the actual JSON wire path.
    let json = serde_json::to_string(&json).unwrap();
    let copy = LoroDoc::new();
    copy.set_peer_id(2).unwrap();
    copy.import_json_updates(json.as_str()).unwrap();
    copy
}

fn map_value(doc: &LoroDoc, key: &str) -> LoroValue {
    match doc.get_map("m").get(key).unwrap() {
        ValueOrContainer::Value(value) => value,
        other => panic!("expected a value, got {other:?}"),
    }
}

fn style_delta(doc: &LoroDoc) -> Option<Vec<loro::TextDelta>> {
    doc.get_deep_value()
        .as_map()
        .unwrap()
        .contains_key("text")
        .then(|| doc.get_text("text").to_delta())
}

fn assert_relay_keeps_prefix(a: &LoroDoc, check_loss: impl Fn(&LoroDoc)) {
    a.commit();
    let prefix = a.export(ExportMode::all_updates()).unwrap();
    let b = json_copy(a);
    check_loss(&b);

    // Full exports must keep working after later relay edits too.
    for tail in 1..=2 {
        b.get_map("relay").insert("extra", tail).unwrap();
        b.commit();
        for mode in [ExportMode::all_updates(), ExportMode::Snapshot] {
            let blob = b.export(mode).unwrap();
            for batch in [false, true] {
                let c = LoroDoc::new();
                c.set_peer_id(3).unwrap();
                c.import(&prefix).unwrap();
                let expected = c.get_deep_value().into_map().unwrap();
                let expected_style = style_delta(&c);
                if batch {
                    c.import_batch(&[blob.clone()]).unwrap();
                } else {
                    c.import(&blob).unwrap();
                }

                let actual = c.get_deep_value().into_map().unwrap();
                for (key, value) in expected.iter() {
                    assert_eq!(actual.get(key), Some(value), "prefix root {key}");
                }
                assert_eq!(actual.len(), expected.len() + 1);
                assert_eq!(actual["relay"]["extra"], LoroValue::from(tail));
                assert_eq!(style_delta(&c), expected_style);
                assert_eq!(c.oplog_vv(), b.oplog_vv());
            }
        }
    }

    // Compare the same representations in the opposite direction: the receiver
    // owns the JSON prefix and the binary sender owns the original values.
    let c = json_copy(a);
    let expected = c.get_deep_value().into_map().unwrap();
    let expected_style = style_delta(&c);
    a.get_map("relay").insert("extra", 3).unwrap();
    a.commit();
    for mode in [ExportMode::all_updates(), ExportMode::Snapshot] {
        let receiver = LoroDoc::new();
        receiver
            .import(&c.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        receiver.import(&a.export(mode).unwrap()).unwrap();
        let actual = receiver.get_deep_value().into_map().unwrap();
        for (key, value) in expected.iter() {
            assert_eq!(actual.get(key), Some(value), "JSON prefix root {key}");
        }
        assert_eq!(style_delta(&receiver), expected_style);
        assert_eq!(actual.len(), expected.len() + 1);
        assert_eq!(actual["relay"]["extra"], LoroValue::I64(3));
        assert_eq!(receiver.oplog_vv(), a.oplog_vv());
    }
}

#[test]
fn binary_map_value_survives_json_then_binary_relay() {
    let a = source();
    a.get_map("m").insert("bin", vec![0u8, 1, 255]).unwrap();
    assert_relay_keeps_prefix(&a, |b| {
        assert_eq!(map_value(b, "bin"), LoroValue::from(vec![0, 1, 255]));
    });
}

#[test]
fn mergeable_text_survives_json_then_binary_relay() {
    let a = source();
    a.get_map("m")
        .ensure_mergeable_text("mt")
        .unwrap()
        .insert(0, "hi")
        .unwrap();
    assert_relay_keeps_prefix(&a, |b| assert!(map_value(b, "mt").is_list()));
}

#[test]
fn mergeable_counter_survives_json_then_binary_relay() {
    let a = source();
    a.get_map("m")
        .ensure_mergeable_counter("mc")
        .unwrap()
        .increment(2.0)
        .unwrap();
    assert_relay_keeps_prefix(&a, |b| assert!(map_value(b, "mc").is_list()));
}

#[test]
fn non_finite_map_values_survive_json_then_binary_relay() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let a = source();
        a.get_map("m").insert("number", value).unwrap();
        assert_relay_keeps_prefix(&a, |b| assert_eq!(map_value(b, "number"), LoroValue::Null));
    }
}

#[test]
fn style_values_survive_json_then_binary_relay() {
    for value in [LoroValue::from(vec![0u8, 255]), LoroValue::Double(f64::NAN)] {
        let a = source();
        let text = a.get_text("text");
        text.insert(0, "styled").unwrap();
        text.mark(0..6, "custom", value).unwrap();
        let expected = text.to_delta();
        assert_relay_keeps_prefix(&a, |b| assert_ne!(b.get_text("text").to_delta(), expected));
    }
}

#[test]
fn nested_and_sequence_values_survive_json_then_binary_relay() {
    let a = source();
    let value = LoroValue::Map(
        vec![(
            "nested".into(),
            LoroValue::List(
                vec![
                    LoroValue::from(vec![1u8, 2]),
                    LoroValue::Double(f64::INFINITY),
                ]
                .into(),
            ),
        )]
        .into(),
    );
    a.get_map("m").insert("nested", value.clone()).unwrap();
    a.get_list("list").push(value.clone()).unwrap();
    let movable = a.get_movable_list("movable");
    movable.push(value.clone()).unwrap();
    movable.push(0).unwrap();
    movable.set(1, value.clone()).unwrap();
    a.get_text("text").insert(0, "nested style").unwrap();
    a.get_text("text")
        .mark(0..6, "custom", value.clone())
        .unwrap();
    assert_relay_keeps_prefix(&a, |b| assert_ne!(map_value(b, "nested"), value));
}

#[test]
fn real_text_conflict_is_rejected_after_json_then_binary_relay() {
    let a = source();
    a.get_text("text").insert(0, "hello").unwrap();
    a.commit();
    let conflict = source();
    conflict.get_text("text").insert(0, "hellO").unwrap();
    conflict.commit();
    let b = json_copy(&conflict);
    b.get_map("relay").insert("extra", 1).unwrap();
    b.commit();

    for mode in [ExportMode::all_updates(), ExportMode::Snapshot] {
        let c = LoroDoc::new();
        c.import(&a.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        let before = c.get_deep_value();
        let vv = c.oplog_vv();
        let err = c.import(&b.export(mode).unwrap()).unwrap_err();
        assert!(matches!(err, LoroError::UsedOpID { id } if id == ID::new(1, 4)));
        assert_eq!(c.get_deep_value(), before);
        assert_eq!(c.oplog_vv(), vv);
    }
}

#[test]
fn genuine_value_conflicts_are_rejected_on_json_and_binary_imports() {
    let nested = |byte| LoroValue::Map(vec![("nested".into(), LoroValue::from(vec![byte]))].into());
    for (local, conflicting) in [
        (LoroValue::from(vec![1u8, 2]), LoroValue::from(vec![1, 3])),
        (LoroValue::Double(f64::NAN), LoroValue::I64(0)),
        (nested(1), nested(2)),
    ] {
        // Check every operation that carries a value, including arena-backed inserts.
        for kind in ["map", "list", "movable-set", "style"] {
            let write = |doc: &LoroDoc, value: LoroValue| match kind {
                "map" => doc.get_map("m").insert("value", value).unwrap(),
                "list" => doc.get_list("list").push(value).unwrap(),
                "movable-set" => {
                    doc.get_movable_list("movable").push(0).unwrap();
                    doc.get_movable_list("movable").set(0, value).unwrap();
                }
                "style" => {
                    doc.get_text("text").insert(0, "style").unwrap();
                    doc.get_text("text").mark(0..5, "custom", value).unwrap();
                }
                _ => unreachable!(),
            };
            let a = source();
            write(&a, local.clone());
            a.commit();
            let conflict = source();
            write(&conflict, conflicting.clone());
            conflict.commit();
            let b = json_copy(&conflict);
            b.get_map("relay").insert("extra", 1).unwrap();
            b.commit();
            let json =
                serde_json::to_string(&b.export_json_updates(&Default::default(), &b.oplog_vv()))
                    .unwrap();
            for import_kind in ["json", "updates", "snapshot"] {
                let c = LoroDoc::new();
                c.import(&a.export(ExportMode::all_updates()).unwrap())
                    .unwrap();
                let before = c.get_deep_value();
                let before_style = style_delta(&c);
                let vv = c.oplog_vv();
                let err = match import_kind {
                    "json" => c.import_json_updates(json.as_str()).unwrap_err(),
                    "updates" => c
                        .import(&b.export(ExportMode::all_updates()).unwrap())
                        .unwrap_err(),
                    "snapshot" => c
                        .import(&b.export(ExportMode::Snapshot).unwrap())
                        .unwrap_err(),
                    _ => unreachable!(),
                };
                assert!(
                    matches!(err, LoroError::UsedOpID { .. }),
                    "{kind}/{import_kind}: {err:?}"
                );
                assert_eq!(c.get_deep_value(), before);
                assert_eq!(style_delta(&c), before_style);
                assert_eq!(c.oplog_vv(), vv);
            }
        }
    }
}
