use loro_internal::{
    configure::{StyleConfig, StyleConfigMap},
    container::richtext::ExpandType,
    cursor::PosType,
    handler::{TextDelta, TextHandler},
    loro::ExportMode,
    LoroDoc, ToJson,
};
use serde_json::json;

fn assert_runs(text: &TextHandler, runs: &[(&str, bool, bool)]) {
    let expected: Vec<_> = runs
        .iter()
        .map(|(insert, bold, italic)| TextDelta::Insert {
            insert: (*insert).into(),
            attributes: {
                let mut attrs = rustc_hash::FxHashMap::default();
                if *bold {
                    attrs.insert("bold".into(), true.into());
                }
                if *italic {
                    attrs.insert("italic".into(), true.into());
                }
                (!attrs.is_empty()).then_some(attrs)
            },
        })
        .collect();
    assert_eq!(text.get_delta(), expected);
    let expected: Vec<_> = runs
        .iter()
        .map(|(insert, bold, italic)| {
            let mut value = json!({"insert": insert});
            let mut attrs = serde_json::Map::new();
            if *bold {
                attrs.insert("bold".into(), json!(true));
            }
            if *italic {
                attrs.insert("italic".into(), json!(true));
            }
            if !attrs.is_empty() {
                value["attributes"] = json!(attrs);
            }
            value
        })
        .collect();
    assert_eq!(text.get_richtext_value().to_json_value(), json!(expected));
}

#[test]
fn richtext_unmark_merges_effective_attributes() {
    for expand in [
        ExpandType::None,
        ExpandType::After,
        ExpandType::Before,
        ExpandType::Both,
    ] {
        // Exact, partial, and wider unmarks, with and without a surviving style.
        for (start, end, runs) in [
            (18, 21, vec![("The ford is upstream.", false)]),
            (
                19,
                21,
                vec![("The ford is upstre", false), ("a", true), ("m.", false)],
            ),
            (18, 20, vec![("The ford is upstream", false), (".", true)]),
            (0, 21, vec![("The ford is upstream.", false)]),
        ] {
            for italic in [false, true] {
                let doc = LoroDoc::new_auto_commit();
                let mut config = StyleConfigMap::new();
                config.insert("bold".into(), StyleConfig::new().expand(expand));
                config.insert("italic".into(), StyleConfig::new().expand(expand));
                doc.config_text_style(config);
                let text = doc.get_text("text");
                text.insert(0, "The ford is upstream.", PosType::Unicode)
                    .unwrap();
                if italic {
                    text.mark(0, 21, "italic", true.into(), PosType::Unicode)
                        .unwrap();
                }
                text.mark(18, 21, "bold", true.into(), PosType::Unicode)
                    .unwrap();
                doc.commit_then_renew();
                text.unmark(start, end, "bold", PosType::Unicode).unwrap();
                doc.commit_then_renew();
                let runs: Vec<_> = runs.iter().map(|(s, bold)| (*s, *bold, italic)).collect();
                assert_runs(&text, &runs);
                let restored =
                    LoroDoc::from_snapshot(&doc.export(ExportMode::Snapshot).unwrap()).unwrap();
                assert_runs(&restored.get_text("text"), &runs);
            }
        }
    }
}
