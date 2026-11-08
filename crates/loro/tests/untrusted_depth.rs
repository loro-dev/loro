//! Untrusted updates must not abort the reader through deep nesting: deep mark values,
//! deep container chains, deep values in snapshot export, and long mergeable names.
//! Everything here runs on a 1 MiB stack. See `context/deep-value-stack-safety.md`.

use loro::{
    ContainerTrait, ExpandType, ExportMode, ImportHistoryMode, LoroDoc, LoroError, LoroMap,
    LoroValue, StyleConfig, StyleConfigMap, VersionVector,
};

fn on_small_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

fn nested_list(depth: usize) -> LoroValue {
    let mut v = LoroValue::I64(1);
    for _ in 0..depth {
        v = LoroValue::List(vec![v].into());
    }
    v
}

fn nested_list_depth(mut v: &LoroValue) -> usize {
    let mut depth = 0;
    while let LoroValue::List(list) = v {
        v = &list[0];
        depth += 1;
    }
    assert_eq!(v, &LoroValue::I64(1));
    depth
}

#[test]
fn equal_deep_values_compare_without_recursion() {
    on_small_stack(|| {
        let a = nested_list(200_000);
        let b = nested_list(200_000);
        assert_eq!(a, b);
        assert_ne!(a, nested_list(199_999));
    });
}

#[test]
fn adjacent_equal_deep_marks_to_delta() {
    on_small_stack(|| {
        let depth = 20_000;
        let doc = LoroDoc::new();
        let mut cfg = StyleConfigMap::new();
        cfg.insert(
            "b".into(),
            StyleConfig {
                expand: ExpandType::None,
            },
        );
        doc.config_text_style(cfg);
        let t = doc.get_text("t");
        t.insert(0, "ab").unwrap();
        // Separately allocated, structurally equal values: `to_delta` compares them.
        t.mark(0..1, "b", nested_list(depth)).unwrap();
        t.mark(1..2, "b", nested_list(depth)).unwrap();
        doc.commit();
        let bytes = doc.export(ExportMode::Snapshot).unwrap();

        let reader = LoroDoc::new();
        reader.import(&bytes).unwrap();
        let delta = reader.get_text("t").to_delta();
        assert_eq!(delta.len(), 1);
    });
}

#[test]
fn snapshot_export_of_deep_values() {
    on_small_stack(|| {
        let depth = 20_000;
        let writer = LoroDoc::new();
        writer.get_map("m").insert("x", nested_list(depth)).unwrap();
        writer.get_list("l").push(nested_list(depth)).unwrap();
        writer
            .get_movable_list("ml")
            .push(nested_list(depth))
            .unwrap();
        writer.commit();
        let updates = writer.export(ExportMode::all_updates()).unwrap();

        let doc = LoroDoc::new();
        doc.import(&updates).unwrap();
        let snapshot = doc.export(ExportMode::Snapshot).unwrap();

        let copy = LoroDoc::new();
        copy.import(&snapshot).unwrap();
        let m = copy.get_map("m").get("x").unwrap().into_value().unwrap();
        assert_eq!(nested_list_depth(&m), depth);
        let l = copy.get_list("l").get(0).unwrap().into_value().unwrap();
        assert_eq!(nested_list_depth(&l), depth);
        let ml = copy
            .get_movable_list("ml")
            .get(0)
            .unwrap()
            .into_value()
            .unwrap();
        assert_eq!(nested_list_depth(&ml), depth);
    });
}

/// JSON updates for a chain of `depth` nested maps under root map `m`, each at key `k`.
fn deep_map_chain_json(depth: usize) -> String {
    let mut ops = Vec::with_capacity(depth);
    for i in 0..depth {
        let container = if i == 0 {
            "cid:root-m:Map".to_string()
        } else {
            format!("cid:{}@0:Map", i - 1)
        };
        ops.push(format!(
            r#"{{"container":"{container}","content":{{"type":"insert","key":"k","value":"🦜:cid:{i}@0:Map"}},"counter":{i}}}"#
        ));
    }
    format!(
        r#"{{"schema_version":1,"start_version":{{}},"peers":["9"],"changes":[{{"id":"0@0","timestamp":0,"deps":[],"lamport":0,"msg":null,"ops":[{}]}}]}}"#,
        ops.join(",")
    )
}

/// Depth of `{ k: { k: ... {} } }`, walked iteratively.
fn map_chain_depth(mut v: &LoroValue) -> usize {
    let mut depth = 0;
    while let Some(next) = v.as_map().unwrap().get("k") {
        v = next;
        depth += 1;
    }
    depth
}

#[test]
fn container_chain_deeper_than_u16() {
    // Container depth used to be a `NonZeroU16`: from 65,535 levels import panicked under
    // the state lock and aborted the process.
    let depth = u16::MAX as usize + 10;
    let json = deep_map_chain_json(depth);
    on_small_stack(move || {
        let writer = LoroDoc::new();
        writer.import_json_updates(json.as_str()).unwrap();
        let updates = writer.export(ExportMode::all_updates()).unwrap();

        let doc = LoroDoc::new();
        doc.import(&updates).unwrap();
        let value = doc.get_deep_value();
        assert_eq!(
            map_chain_depth(value.as_map().unwrap().get("m").unwrap()),
            depth
        );

        // A fast snapshot loads lazily, so its chain reaches readers without import ever
        // walking it; the readers themselves must be iterative.
        let snapshot = doc.export(ExportMode::Snapshot).unwrap();
        let copy = LoroDoc::new();
        copy.import(&snapshot).unwrap();
        let value = copy.get_deep_value();
        assert_eq!(
            map_chain_depth(value.as_map().unwrap().get("m").unwrap()),
            depth
        );
        let with_id = copy.get_deep_value_with_id();
        let mut v = with_id.as_map().unwrap().get("m").unwrap();
        let mut seen = 0;
        loop {
            let inner = v.as_map().unwrap().get("value").unwrap().as_map().unwrap();
            match inner.get("k") {
                Some(next) => {
                    v = next;
                    seen += 1;
                }
                None => break,
            }
        }
        assert_eq!(seen, depth);
        assert_eq!(copy.oplog_vv(), doc.oplog_vv());
    });
}

/// JSON updates writing `y = 1` into the mergeable map `🤝:$t>a>a>…` with `keys` keys.
fn long_mergeable_name_json(keys: usize) -> String {
    let path = vec!["a"; keys].join(">");
    format!(
        r#"{{"schema_version":1,"start_version":{{}},"peers":["9"],"changes":[{{"id":"0@0","timestamp":0,"deps":[],"lamport":0,"msg":null,"ops":[{{"container":"cid:root-🤝:$t>{path}:Map","content":{{"type":"insert","key":"y","value":1}},"counter":0}}]}}]}}"#
    )
}

#[test]
fn long_mergeable_names_import_in_bounded_time() {
    for keys in [100, 20_000] {
        let json = long_mergeable_name_json(keys);
        on_small_stack(move || {
            let writer = LoroDoc::new();
            writer.detach();
            writer.import_json_updates(json.as_str()).unwrap();
            let updates = writer.export(ExportMode::all_updates()).unwrap();

            let start = std::time::Instant::now();
            let doc = LoroDoc::new();
            doc.detach();
            doc.import(&updates).unwrap();
            doc.try_checkout_to_latest().unwrap();
            let json = doc
                .export_json_updates(&VersionVector::default(), &doc.oplog_vv())
                .unwrap();
            let id = json.changes[0].ops[0].container.clone();
            // Within the ancestor-name budget the name stays mergeable; 20,000 keys exceed
            // it, so that name is an ordinary (if oddly named) root, registered in one step.
            assert_eq!(id.is_mergeable(), keys == 100, "keys = {keys}");
            assert!(
                start.elapsed() < std::time::Duration::from_secs(5),
                "keys = {keys}: {:?}",
                start.elapsed()
            );
        });
    }
}

#[test]
fn ensure_mergeable_fails_beyond_the_name_budget() {
    on_small_stack(|| {
        let doc = LoroDoc::new();
        let mut map: LoroMap = doc.get_map("t");
        // Long keys reach the ancestor-name budget in few levels: level `i` adds an
        // ancestor name of about `i * key.len()` bytes.
        // Keep every name under 64 KB: longer container IDs do not survive a snapshot
        // round trip (a separate KV-store limit).
        let key = "a".repeat(1024);
        let mut levels = 0;
        let err = loop {
            match map.ensure_mergeable_map(&key) {
                Ok(child) => {
                    assert!(child.id().is_mergeable());
                    map = child;
                    levels += 1;
                    assert!(levels < 64, "the budget caps nesting");
                }
                Err(e) => break e,
            }
        };
        assert!(levels > 16, "levels = {levels}");
        assert!(err.to_string().contains("nested too deeply"), "{err}");
        doc.commit();
        // The failed call left nothing behind; the document still round-trips.
        let copy = LoroDoc::new();
        copy.import(&doc.export(ExportMode::Snapshot).unwrap())
            .unwrap();
        assert_eq!(copy.get_deep_value(), doc.get_deep_value());
    });
}

/// The known-history comparison for `ImportHistoryMode::JsonLossy` walked values
/// recursively. It also runs on repeated imports, which aborted the process here.
#[test]
fn json_lossy_reimport_compares_deep_values_without_recursion() {
    on_small_stack(|| {
        let doc = LoroDoc::new();
        doc.set_peer_id(1).unwrap();
        doc.get_map("m").insert("v", nested_list(20_000)).unwrap();
        doc.commit();
        let known = doc.export(ExportMode::all_updates()).unwrap();
        doc.get_map("m").insert("new", 1).unwrap();
        doc.commit();
        let with_new_op = doc.export(ExportMode::all_updates()).unwrap();

        let reader = LoroDoc::new();
        reader.import(&known).unwrap();
        for mode in [ImportHistoryMode::JsonLossy, ImportHistoryMode::Exact] {
            let status = reader.import_with_history_mode(&known, "", mode).unwrap();
            assert!(status.success.is_empty() && status.pending.is_none());
        }
        let status = reader
            .import_with_history_mode(&with_new_op, "", ImportHistoryMode::JsonLossy)
            .unwrap();
        assert!(!status.success.is_empty());
    });
}

#[test]
fn json_lossy_reimport_reports_a_conflict_deep_in_a_value() {
    on_small_stack(|| {
        let write = |value: LoroValue| {
            let doc = LoroDoc::new();
            doc.set_peer_id(1).unwrap();
            doc.get_map("m").insert("v", value).unwrap();
            doc.commit();
            doc.export(ExportMode::all_updates()).unwrap()
        };
        let reader = LoroDoc::new();
        reader.import(&write(nested_list(20_000))).unwrap();
        let err = reader
            .import_with_history_mode(
                &write(nested_list(20_001)),
                "",
                ImportHistoryMode::JsonLossy,
            )
            .unwrap_err();
        assert!(matches!(err, LoroError::UsedOpID { .. }));
    });
}
