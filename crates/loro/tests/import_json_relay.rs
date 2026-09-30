//! History relayed through JSON keeps its lossy values even after binary export.
//! Known prefixes must be compared with that loss in mind, then trimmed without
//! replacing the receiver's original values.

use loro::{
    ContainerID, ContainerType, ExpandType, ExportMode, JsonFutureOp, JsonOpContent, LoroDoc,
    LoroError, LoroValue, StyleConfig, ValueOrContainer, ID,
};

fn source() -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.config_default_text_style(Some(StyleConfig {
        expand: ExpandType::After,
    }));
    doc
}

fn js_json(doc: &LoroDoc) -> String {
    let json = doc.export_json_updates(&Default::default(), &doc.oplog_vv());
    let mut json = serde_json::to_value(&json).unwrap();
    // Model JS numbers / JSON.stringify: 2.0 is written as 2. A typed JsonSchema
    // or a Rust-only string round trip would preserve the Double in this case.
    fn canonicalize_numbers(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Number(number) => {
                let n = number.as_f64().unwrap();
                if number.is_f64() && n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0 {
                    *value = serde_json::Value::from(n as i64);
                }
            }
            serde_json::Value::Array(values) => values.iter_mut().for_each(canonicalize_numbers),
            serde_json::Value::Object(values) => values.values_mut().for_each(canonicalize_numbers),
            _ => {}
        }
    }
    canonicalize_numbers(&mut json);
    serde_json::to_string(&json).unwrap()
}

fn json_copy(doc: &LoroDoc) -> LoroDoc {
    let json = js_json(doc);
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
        for mode in [
            Some(ExportMode::all_updates()),
            Some(ExportMode::Snapshot),
            None,
        ] {
            let blob = mode.map(|mode| b.export(mode).unwrap());
            for batch in [false, true] {
                if batch && blob.is_none() {
                    continue;
                }
                let c = LoroDoc::new();
                c.set_peer_id(3).unwrap();
                c.import(&prefix).unwrap();
                let expected = c.get_deep_value().into_map().unwrap();
                let expected_style = style_delta(&c);
                let prefix_vv = c.oplog_vv();
                let expected_history =
                    serde_json::to_string(&c.export_json_updates_without_peer_compression(
                        &Default::default(),
                        &prefix_vv,
                    ))
                    .unwrap();
                match &blob {
                    Some(blob) if batch => {
                        c.import_batch(&[blob.clone()]).unwrap();
                    }
                    Some(blob) => {
                        c.import(blob).unwrap();
                    }
                    None => {
                        c.import_json_updates(js_json(&b).as_str()).unwrap();
                    }
                }

                let actual = c.get_deep_value().into_map().unwrap();
                for (key, value) in expected.iter() {
                    assert_eq!(actual.get(key), Some(value), "prefix root {key}");
                }
                assert_eq!(actual.len(), expected.len() + 1);
                assert_eq!(actual["relay"]["extra"], LoroValue::from(tail));
                assert_eq!(style_delta(&c), expected_style);
                assert_eq!(
                    serde_json::to_string(&c.export_json_updates_without_peer_compression(
                        &Default::default(),
                        &prefix_vv
                    ))
                    .unwrap(),
                    expected_history
                );
                assert_eq!(c.oplog_vv(), b.oplog_vv());
            }
        }
    }

    // Compare the same representations in the opposite direction: the receiver
    // owns the JSON prefix and the binary sender owns the original values.
    let c = json_copy(a);
    let expected = c.get_deep_value().into_map().unwrap();
    let expected_style = style_delta(&c);
    let prefix_vv = c.oplog_vv();
    let expected_history = serde_json::to_string(
        &c.export_json_updates_without_peer_compression(&Default::default(), &prefix_vv),
    )
    .unwrap();
    a.get_map("relay").insert("extra", 3).unwrap();
    a.commit();
    for mode in [
        Some(ExportMode::all_updates()),
        Some(ExportMode::Snapshot),
        None,
    ] {
        let receiver = LoroDoc::new();
        receiver
            .import(&c.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        if let Some(mode) = mode {
            receiver.import(&a.export(mode).unwrap()).unwrap();
        } else {
            receiver.import_json_updates(js_json(a).as_str()).unwrap();
        }
        let actual = receiver.get_deep_value().into_map().unwrap();
        for (key, value) in expected.iter() {
            assert_eq!(actual.get(key), Some(value), "JSON prefix root {key}");
        }
        assert_eq!(style_delta(&receiver), expected_style);
        assert_eq!(
            serde_json::to_string(
                &receiver
                    .export_json_updates_without_peer_compression(&Default::default(), &prefix_vv)
            )
            .unwrap(),
            expected_history
        );
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
fn integral_double_survives_json_and_binary_relays() {
    for number in [2.0, -2.0, -0.0] {
        let a = source();
        a.get_map("m")
            .insert("number", LoroValue::Double(number))
            .unwrap();
        assert_relay_keeps_prefix(&a, |b| {
            assert_eq!(map_value(b, "number"), LoroValue::I64(number as i64));
        });
    }
}

#[test]
fn integral_double_json_reimport_into_original_writer_succeeds() {
    let a = source();
    a.get_map("m")
        .insert("number", LoroValue::Double(2.0))
        .unwrap();
    a.commit();
    let b = json_copy(&a);
    assert_eq!(map_value(&b, "number"), LoroValue::I64(2));
    b.get_map("relay").insert("extra", 1).unwrap();
    b.commit();
    a.import_json_updates(js_json(&b).as_str()).unwrap();
    assert_eq!(map_value(&a, "number"), LoroValue::Double(2.0));
    assert_eq!(a.get_map("relay").get_value()["extra"], LoroValue::I64(1));
    assert_eq!(a.oplog_vv(), b.oplog_vv());
}

#[test]
fn container_marker_string_survives_json_and_binary_relays() {
    let a = source();
    // The string uses compressed peer index 0, which the JSON importer maps to
    // actual peer 1. Its counter matches the map op, so this is accepted JSON.
    a.get_map("m").insert("cid", "🦜:cid:0@0:Text").unwrap();
    assert_relay_keeps_prefix(&a, |b| {
        assert!(matches!(
            b.get_map("m").get("cid"),
            Some(ValueOrContainer::Container(_))
        ));
        assert_eq!(
            b.get_map("m").get_value()["cid"],
            LoroValue::Container(ContainerID::new_normal(ID::new(1, 0), ContainerType::Text))
        );
    });
}

#[test]
fn tagged_counter_values_keep_float_semantics_through_json() {
    let a = source();
    for increment in [2.0, -0.25, 0.5] {
        a.get_counter("counter").increment(increment).unwrap();
        a.commit();
    }
    assert_relay_keeps_prefix(&a, |b| {
        assert_eq!(b.get_counter("counter").get_value(), 2.25)
    });
}

#[test]
fn non_finite_counter_json_remains_invalid_before_history_comparison() {
    for increment in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let a = source();
        a.get_counter("counter").increment(increment).unwrap();
        a.commit();
        let b = source();
        let before = b.oplog_vv();
        assert!(matches!(
            b.import_json_updates(js_json(&a).as_str()),
            Err(LoroError::InvalidJsonSchema)
        ));
        assert_eq!(b.oplog_vv(), before);
    }
}

#[test]
fn unknown_op_payload_survives_json_and_binary_relays() {
    let template = source();
    template.get_counter("unknown").increment(1.0).unwrap();
    template.commit();
    let mut json = template
        .export_json_updates_without_peer_compression(&Default::default(), &template.oplog_vv());
    let op = &mut json.changes[0].ops[0];
    op.container = ContainerID::new_root("unknown", ContainerType::Unknown(9));
    let JsonOpContent::Future(future) = &mut op.content else {
        unreachable!()
    };
    // A newer container can carry arbitrary nested LoroValues in its opaque op.
    // The typed schema preserves Double(2.0); a JS relay turns it into I64(2).
    future.value = JsonFutureOp::Unknown(
        serde_json::from_str(r#"{"value_type":"loro_value","value":{"nested":[2.0]}}"#).unwrap(),
    );
    let a = source();
    a.import_json_updates(json).unwrap();
    let original = js_json(&a);
    assert_relay_keeps_prefix(&a, |b| assert_eq!(js_json(b), original));
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
                    LoroValue::Double(2.0),
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
        (LoroValue::Double(2.0), LoroValue::I64(3)),
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
