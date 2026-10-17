use loro::{ExpandType, ExportMode, LoroDoc, LoroValue, StyleConfig, StyleConfigMap};

fn source(depth: usize) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(1198).unwrap();
    let mut config = StyleConfigMap::new();
    config.insert(
        "deep".into(),
        StyleConfig {
            expand: ExpandType::After,
        },
    );
    config.insert(
        "dead".into(),
        StyleConfig {
            expand: ExpandType::After,
        },
    );
    doc.config_text_style(config);
    let text = doc.get_text("text");
    text.insert(0, "hello!").unwrap();
    let mut value = LoroValue::I64(42);
    for i in 0..depth {
        value = if i % 2 == 0 {
            LoroValue::List(vec![value].into())
        } else {
            LoroValue::Map(vec![("a".into(), value)].into())
        };
    }
    text.mark(0..5, "deep", value.clone()).unwrap();
    text.mark(5..6, "dead", value).unwrap();
    text.delete(5, 1).unwrap();
    doc.get_map("unrelated").insert("ok", true).unwrap();
    doc.commit();
    doc
}

fn check(doc: &LoroDoc, depth: usize) {
    let text = doc.get_text("text");
    assert_eq!(text.to_string(), "hello");
    let rich = text.get_richtext_value();
    let mut value = &rich[0]["attributes"]["deep"];
    for i in (0..depth).rev() {
        value = if i % 2 == 0 { &value[0] } else { &value["a"] };
    }
    assert_eq!(value, &LoroValue::I64(42));
    assert_eq!(doc.state_frontiers(), doc.oplog_frontiers());
    assert!(doc.get_map("unrelated").get("ok").is_some());
    doc.get_map("unrelated").insert("edit", 1).unwrap();
    doc.commit();
    assert!(!doc.export(ExportMode::Snapshot).unwrap().is_empty());
}

#[test]
fn mark_snapshot_reads_above_the_external_serde_limit() {
    for depth in [128, 129, 200] {
        let bytes = source(depth).export(ExportMode::Snapshot).unwrap();
        let doc = LoroDoc::new();
        doc.import(&bytes).unwrap();
        check(&doc, depth);
    }
}

#[test]
fn mark_shallow_and_state_only_keep_deep_live_values() {
    for depth in [129, 200] {
        let doc = source(depth);
        let frontiers = doc.oplog_frontiers();
        for mode in [
            ExportMode::shallow_snapshot(&frontiers),
            ExportMode::state_only(None),
        ] {
            let bytes = doc.export(mode).unwrap();
            let other = LoroDoc::new();
            other.import(&bytes).unwrap();
            check(&other, depth);
        }
    }
}

#[test]
fn deep_mark_roundtrips_on_one_mib_stack() {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            for depth in [129, 200, 20_000] {
                let doc = source(depth);
                let frontiers = doc.oplog_frontiers();
                for mode in [
                    ExportMode::Snapshot,
                    ExportMode::shallow_snapshot(&frontiers),
                    ExportMode::state_only(None),
                ] {
                    let bytes = doc.export(mode).unwrap();
                    let other = LoroDoc::new();
                    other.import(&bytes).unwrap();
                    check(&other, depth);
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn remote_binary_mark_updates_remain_readable_after_snapshot_roundtrip() {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            for depth in [129, 200, 20_000] {
                let source = source(depth);
                let updates = source.export(ExportMode::all_updates()).unwrap();
                let remote = LoroDoc::new();
                remote.import(&updates).unwrap();
                check(&remote, depth);
                let snapshot = remote.export(ExportMode::Snapshot).unwrap();
                let copy = LoroDoc::new();
                copy.import(&snapshot).unwrap();
                check(&copy, depth);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
