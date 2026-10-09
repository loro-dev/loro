use loro::{ExpandType, LoroDoc, StyleConfig, StyleConfigMap, TextDelta};

fn assert_fork_keeps_styles(fork: &LoroDoc) {
    let text = fork.get_text("text");
    // Neither key was used in the source's history, so snapshot decoding
    // cannot infer its config from existing marks.
    text.mark(0..2, "new_custom_key", true).unwrap();
    text.mark(0..2, "explicit", true).unwrap();
    text.insert(0, "L").unwrap();
    text.insert(text.len_unicode(), "R").unwrap();
    assert_eq!(
        text.to_delta(),
        vec![
            TextDelta::Insert {
                insert: "L".into(),
                attributes: Some(
                    [("new_custom_key".into(), true.into())]
                        .into_iter()
                        .collect(),
                ),
            },
            TextDelta::Insert {
                insert: "ab".into(),
                attributes: Some(
                    [
                        ("new_custom_key".into(), true.into()),
                        ("explicit".into(), true.into())
                    ]
                    .into_iter()
                    .collect(),
                ),
            },
            TextDelta::Insert {
                insert: "R".into(),
                attributes: None,
            },
        ]
    );
}

#[test]
fn fork_and_fork_at_keep_default_and_explicit_text_styles() {
    let doc = LoroDoc::new();
    let mut styles = StyleConfigMap::new();
    styles.insert(
        "explicit".into(),
        StyleConfig::new().expand(ExpandType::None),
    );
    doc.config_text_style(styles);
    doc.config_default_text_style(Some(StyleConfig::new().expand(ExpandType::Before)));
    doc.get_text("text").insert(0, "ab").unwrap();
    doc.commit();
    let version = doc.state_frontiers();
    doc.get_map("later").insert("value", 1).unwrap();
    doc.commit();

    let fork = doc.fork();
    let historical = doc.fork_at(&version).unwrap();
    doc.checkout(&version).unwrap();
    let detached = doc.fork();
    doc.checkout_to_latest();

    // Copies must also be independent of later source config changes.
    doc.config_default_text_style(None);
    doc.config_text_style(StyleConfigMap::new());
    for fork in [&fork, &historical, &detached] {
        assert_fork_keeps_styles(fork);
    }
}

#[test]
fn source_and_fork_accept_a_previously_unused_style_key() {
    let doc = LoroDoc::new();
    doc.config_default_text_style(Some(StyleConfig::new()));
    doc.get_text("text").insert(0, "ab").unwrap();
    doc.commit();
    let fork = doc.fork();
    let historical = doc.fork_at(&doc.state_frontiers()).unwrap();
    for doc in [&doc, &fork, &historical] {
        doc.get_text("text").mark(0..2, "unused", true).unwrap();
    }
}

#[test]
fn wasm_facing_internal_document_forks_keep_default_text_style() {
    // The WASM wrapper delegates both fork methods directly to this Rust API.
    use loro_internal::{cursor::PosType, LoroDoc as InternalDoc};

    let doc = InternalDoc::new_auto_commit();
    doc.config_default_text_style(Some(StyleConfig::new()));
    doc.get_text("text")
        .insert(0, "ab", PosType::Unicode)
        .unwrap();
    doc.commit_then_renew();
    let fork = doc.fork();
    let historical = doc.fork_at(&doc.state_frontiers()).unwrap();
    for doc in [&doc, &fork, &historical] {
        doc.get_text("text")
            .mark(0, 2, "unused", true.into(), PosType::Unicode)
            .unwrap();
    }
}
