use std::sync::Arc;

use loro::{
    LoroDoc, LoroError, LoroList, LoroMap, LoroResult, LoroText, LoroValue, StyleConfigMap, ToJson,
    UndoManager,
};
use loro_internal::{configure::StyleConfig, id::ID, loro::CommitOptions};
use serde_json::json;
use tracing::{debug_span, info_span};

#[test]
fn basic_list_undo_insertion() -> Result<(), LoroError> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let mut undo = UndoManager::new(&doc);
    let list = doc.get_list("list");
    list.push("12")?;
    doc.commit();
    list.push("34")?;
    doc.commit();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "list": ["12", "34"]
        })
    );
    undo.undo(&doc)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "list": ["12"]
        })
    );
    undo.undo(&doc)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "list": []
        })
    );

    Ok(())
}

#[test]
fn basic_list_undo_deletion() -> Result<(), LoroError> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let list = doc.get_list("list");
    let mut undo = UndoManager::new(&doc);
    list.push("12")?; // op 0
    doc.commit();
    list.push("34")?; // op 1
    doc.commit();
    list.delete(1, 1)?; // op 2
    doc.commit();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "list": ["12"]
        })
    );
    undo.undo(&doc)?; // op 3
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "list": ["12", "34"]
        })
    );

    // Now, to undo "34" correctly we need to include the latest change
    // If we only undo op 1, op 3 will create "34" again.
    undo.undo(&doc)?; // op 4
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "list": ["12"]
        })
    );

    assert_eq!(doc.oplog_frontiers()[0].counter, 4);

    Ok(())
}

#[test]
fn basic_map_undo() -> Result<(), LoroError> {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1)?;
    let mut undo = UndoManager::new(&doc_a);
    doc_a.get_map("map").insert("a", "a")?;
    doc_a.commit();
    doc_a.get_map("map").insert("b", "b")?;
    doc_a.commit();
    doc_a.get_map("map").delete("a")?;
    doc_a.commit();
    undo.undo(&doc_a)?; // op 3
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({"map": {"a": "a", "b": "b"}})
    );

    undo.undo(&doc_a)?; // op 4
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({"map": {"a": "a"}})
    );

    undo.undo(&doc_a)?; // op 5
    assert_eq!(doc_a.get_deep_value().to_json_value(), json!({"map": {}}));

    // Redo
    undo.redo(&doc_a)?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({"map": {
            "a": "a"
        }})
    );

    // Redo
    undo.redo(&doc_a)?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({"map": {
            "a": "a",
            "b": "b"
        }})
    );

    // Redo
    undo.redo(&doc_a)?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({"map": {
            "b": "b"
        }})
    );

    Ok(())
}

#[test]
fn map_collaborative_undo() -> Result<(), LoroError> {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1)?;
    let mut undo = UndoManager::new(&doc_a);
    doc_a.get_map("map").insert("a", "a")?;
    doc_a.commit();

    let doc_b = LoroDoc::new();
    doc_b.import(&doc_a.export_from(&Default::default()))?;
    doc_b.get_map("map").insert("b", "b")?;
    doc_b.commit();

    doc_a.import(&doc_b.export_from(&Default::default()))?;
    undo.undo(&doc_a)?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({"map": {"b": "b"}})
    );
    Ok(())
}

#[test]
fn map_container_undo() -> Result<(), LoroError> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let mut undo = UndoManager::new(&doc);
    let map = doc.get_map("map");
    let text = map.insert_container("text", LoroText::new())?; // op 0
    doc.commit();
    text.insert(0, "T")?; // op 1
    doc.commit();
    map.insert("number", 0)?; // op 2
    doc.commit();
    undo.undo(&doc)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({"map": {"text": "T"}})
    );
    undo.undo(&doc)?;
    undo.undo(&doc)?;
    assert_eq!(doc.get_deep_value().to_json_value(), json!({"map": {}}));
    undo.redo(&doc)?;
    undo.redo(&doc)?;
    undo.redo(&doc)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({"map": {"text": "T", "number": 0}})
    );
    Ok(())
}

/// This test case matches the example given here
///
/// [PLF23] Extending Automerge: Undo, Redo, and Move
/// Leo Stewen, Martin Kleppmann, Liangrun Da
/// https://youtu.be/uP7AKExkMGU?si=TR2JHRdmAitOVaMw&t=768
///
///
///      ┌─A-Set───┐ ┌─B-set   ┌──A-undo   ┌─A-redo
///      │         │ │     │   │        │  │      │
///      │         │ │     │   │        │  │      │
///      │         ▼ │     ▼   │        ▼  │      ▼
/// ┌────┴────┐ ┌────┴─┐ ┌─────┴──┐ ┌──────┴┐ ┌──────┐
/// │         │ │      │ │        │ │       │ │      │
/// │  Black  │ │ Red  │ │  Green │ │ Black │ │Green │
/// │         │ │      │ │        │ │       │ │      │
/// └─────────┘ └──────┘ └────────┘ └───────┘ └──────┘
///
/// It's also how the following products implement undo/redo
/// - Google Sheet
/// - Google Slides
/// - Figma
/// - Microsoft Powerpoint
/// - Excel
#[test]
fn one_register_collaborative_undo() -> Result<(), LoroError> {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1)?;
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2)?;
    doc_a.get_map("map").insert("color", "black")?;
    sync(&doc_a, &doc_b);
    let mut undo = UndoManager::new(&doc_a);
    doc_a.get_map("map").insert("color", "red")?;
    undo.record_new_checkpoint(&doc_a)?;
    sync(&doc_a, &doc_b);
    doc_b.get_map("map").insert("color", "green")?;
    sync(&doc_a, &doc_b);
    undo.record_new_checkpoint(&doc_a)?;
    undo.undo(&doc_a)?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({"map": {"color": "black"}})
    );
    undo.redo(&doc_a)?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({"map": {"color": "green"}})
    );
    Ok(())
}

fn sync(a: &LoroDoc, b: &LoroDoc) {
    a.import(&b.export_from(&a.oplog_vv())).unwrap();
    b.import(&a.export_from(&b.oplog_vv())).unwrap();
}

#[test]
fn undo_id_span_that_contains_remote_deps_inside() -> Result<(), LoroError> {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1)?;
    let mut undo_a = UndoManager::new(&doc_a);
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2)?;

    //                                      ┌──────────────┐
    //                                      │              │
    // Op from B            ┌───────────────┤  Delete "A"  │◄─────────────────────┐
    //                      │               │  Insert "B"  │                      │
    //                      │               │              │                      │
    //                      │               └──────────────┘                      │
    //                      │                                                     │
    //                      ▼                                                     │
    //             ┌────────────────┐       ┌──────────────────┐          ┌───────┴──────────┐
    //             │                │       │                  │          │                  │
    // Ops from A  │   Insert "A"   │◄──────┤ Insert " rules"  │◄─────────┤   Insert "."     │
    //             │                │       │                  │          │                  │
    //             └────────────────┘       └──────────────────┘          └──────────────────┘

    doc_a.get_text("text").insert(0, "A")?;
    sync(&doc_a, &doc_b);
    doc_b.get_text("text").insert(0, "B")?;
    doc_b.get_text("text").delete(1, 1)?;
    doc_a.get_text("text").insert(1, " rules")?;
    sync(&doc_a, &doc_b);
    doc_a.get_text("text").insert(7, ".")?;
    //                                     ┌──────────────┐
    //                                     │              │
    //  Op from B          ┌───────────────┤  Delete "A"  │◄─────────────────────┐
    //  should not be      │               │  Insert "B"  │                      │
    //  undone             │               │              │                      │
    //                     │               ├──────────────┤                      │
    //        ┌────────────┴───────────────┴──────────────┴──────────────────────┼──────────────┐
    //        │            │                                                     │              │
    //        │   ┌────────────────┐       ┌──────────────────┐          ┌───────┴──────────┐   │
    //        │   │                │       │                  │          │                  │   │
    //  Undo  │   │   Insert "A"   │◄──────┤ Insert " rules"  │◄─────────┤   Insert "."     │   │
    // These  │   │                │       │                  │          │                  │   │
    //        │   └────────────────┘       └──────────────────┘          └──────────────────┘   │
    //        │                                                                                 │
    //        └─────────────────────────────────────────────────────────────────────────────────┘
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({
            "text": "B rules."
        })
    );
    while undo_a.can_undo() {
        undo_a.undo(&doc_a)?;
    }
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({
            "text": "B"
        })
    );
    assert_eq!(doc_a.oplog_frontiers()[0].counter, 14);
    Ok(())
}

#[test]
fn undo_id_span_that_contains_remote_deps_inside_many_times() -> Result<(), LoroError> {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1)?;
    let mut undo = UndoManager::new(&doc_a);
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2)?;

    const TIMES: usize = 10;
    // Replay 10 times
    for _ in 0..TIMES {
        doc_a.get_text("text").insert(0, "A")?;
        sync(&doc_a, &doc_b);
        doc_b.get_text("text").insert(0, "B")?;
        doc_b.get_text("text").delete(1, 1)?;
        doc_a.get_text("text").insert(1, " rules")?;
        sync(&doc_a, &doc_b);
        doc_a.get_text("text").insert(7, ".")?;
    }

    // Undo all ops from A
    while undo.can_undo() {
        undo.undo(&doc_a)?;
    }
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({
            "text": "B".repeat(TIMES )
        })
    );
    Ok(())
}

#[test]
fn undo_manager() -> Result<(), LoroError> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let mut undo = UndoManager::new(&doc);
    doc.get_text("text").insert(0, "123")?;
    undo.record_new_checkpoint(&doc)?;
    doc.get_text("text").insert(3, "456")?;
    undo.record_new_checkpoint(&doc)?;
    doc.get_text("text").insert(6, "789")?;
    undo.record_new_checkpoint(&doc)?;
    for i in 0..10 {
        info_span!("round", i).in_scope(|| {
            assert_eq!(doc.get_text("text").to_string(), "123456789");
            undo.undo(&doc)?;
            assert_eq!(doc.get_text("text").to_string(), "123456");
            undo.undo(&doc)?;
            assert_eq!(doc.get_text("text").to_string(), "123");
            undo.undo(&doc)?;
            assert_eq!(doc.get_text("text").to_string(), "");
            undo.redo(&doc)?;
            assert_eq!(doc.get_text("text").to_string(), "123");
            undo.redo(&doc)?;
            assert_eq!(doc.get_text("text").to_string(), "123456");
            undo.redo(&doc)?;
            assert_eq!(doc.get_text("text").to_string(), "123456789");
            Ok::<(), loro::LoroError>(())
        })?;
    }

    Ok(())
}

#[test]
fn undo_manager_with_sub_container() -> Result<(), LoroError> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let mut undo = UndoManager::new(&doc);
    let map = doc.get_list("list").insert_container(0, LoroMap::new())?;
    undo.record_new_checkpoint(&doc)?;
    let text = map.insert_container("text", LoroText::new())?;
    undo.record_new_checkpoint(&doc)?;
    text.insert(0, "123")?;
    undo.record_new_checkpoint(&doc)?;
    for i in 0..10 {
        info_span!("round", ?i).in_scope(|| {
            assert_eq!(
                doc.get_deep_value().to_json_value(),
                json!({
                    "list": [{
                        "text": "123"
                    }]
                })
            );
            undo.undo(&doc)?;
            assert_eq!(
                doc.get_deep_value().to_json_value(),
                json!({
                    "list": [{
                        "text": ""
                    }]
                })
            );
            undo.undo(&doc)?;
            assert_eq!(
                doc.get_deep_value().to_json_value(),
                json!({
                    "list": [{}]
                })
            );
            undo.undo(&doc)?;
            assert_eq!(
                doc.get_deep_value().to_json_value(),
                json!({
                    "list": []
                })
            );
            undo.redo(&doc)?;
            assert_eq!(
                doc.get_deep_value().to_json_value(),
                json!({
                    "list": [{}]
                })
            );
            undo.redo(&doc)?;
            assert_eq!(
                doc.get_deep_value().to_json_value(),
                json!({
                    "list": [{
                        "text": ""
                    }]
                })
            );
            undo.redo(&doc)?;
            assert_eq!(
                doc.get_deep_value().to_json_value(),
                json!({
                    "list": [{
                        "text": "123"
                    }]
                })
            );

            Ok::<(), loro::LoroError>(())
        })?;
    }

    Ok::<(), loro::LoroError>(())
}

#[test]
fn test_undo_container_deletion() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let mut undo = UndoManager::new(&doc);

    let map = doc.get_map("map");
    let text = map.insert_container("text", LoroText::new())?;
    undo.record_new_checkpoint(&doc)?;
    text.insert(0, "T")?;
    undo.record_new_checkpoint(&doc)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({"map": {"text": "T"}})
    );
    map.delete("text")?;
    assert_eq!(doc.get_deep_value().to_json_value(), json!({"map": {}}));
    undo.record_new_checkpoint(&doc)?;
    undo.undo(&doc)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({"map": {"text": "T"}})
    );
    undo.redo(&doc)?;
    assert_eq!(doc.get_deep_value().to_json_value(), json!({"map": {}}));
    undo.undo(&doc)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({"map": {"text": "T"}})
    );
    undo.redo(&doc)?;
    assert_eq!(doc.get_deep_value().to_json_value(), json!({"map": {}}));
    doc.commit();
    Ok(())
}

#[test]
fn test_richtext_checkout() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let text = doc.get_text("text");
    text.insert(0, "Hello")?; // op 0-5
    text.mark(0..5, "bold", true)?; // op 5-7
    text.unmark(0..5, "bold")?; // op 7-9
    text.delete(0, 5)?;
    doc.commit();

    doc.subscribe_root(Arc::new(|event| {
        dbg!(&event);
        let t = event.events[0].diff.as_text().unwrap();
        let i = t[0].as_insert().unwrap();
        let style = i.1.as_ref().unwrap().get("bold").unwrap();
        assert_eq!(style, &LoroValue::Bool(true));
    }));
    doc.checkout(&ID::new(1, 6).into())?;
    assert_eq!(
        text.to_delta().to_json_value(),
        json!([{"insert": "Hello", "attributes": {"bold": true}}])
    );
    Ok(())
}

#[test]
fn undo_richtext_editing() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let mut undo = UndoManager::new(&doc);
    let text = doc.get_text("text");
    text.insert(0, "Hello")?;
    undo.record_new_checkpoint(&doc)?;
    text.mark(0..5, "bold", true)?;
    undo.record_new_checkpoint(&doc)?;
    assert_eq!(
        text.to_delta().to_json_value(),
        json!([
            {"insert": "Hello", "attributes": {"bold": true}}
        ])
    );
    for i in 0..10 {
        debug_span!("round", i).in_scope(|| {
            undo.undo(&doc)?;
            assert_eq!(
                text.to_delta().to_json_value(),
                json!([
                    {"insert": "Hello", }
                ])
            );
            undo.undo(&doc)?;
            assert_eq!(text.to_delta().to_json_value(), json!([]));
            debug_span!("redo 1").in_scope(|| {
                undo.redo(&doc).unwrap();
            });
            assert_eq!(
                text.to_delta().to_json_value(),
                json!([
                    {"insert": "Hello", }
                ])
            );
            debug_span!("redo 2").in_scope(|| {
                undo.redo(&doc).unwrap();
            });
            assert_eq!(
                text.to_delta().to_json_value(),
                json!([
                    {"insert": "Hello", "attributes": {"bold": true}}
                ])
            );

            Ok::<(), loro::LoroError>(())
        })?;
    }
    Ok(())
}

#[test]
fn undo_richtext_editing_collab() -> LoroResult<()> {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1)?;
    let mut undo = UndoManager::new(&doc_a);
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2)?;
    doc_a.get_text("text").insert(0, "A fox jumped")?;
    undo.record_new_checkpoint(&doc_a)?;
    sync(&doc_a, &doc_b);
    doc_b.get_text("text").mark(2..12, "italic", true)?;
    sync(&doc_a, &doc_b);
    doc_a.get_text("text").mark(0..5, "bold", true)?;
    undo.record_new_checkpoint(&doc_a)?;
    sync(&doc_a, &doc_b);
    assert_eq!(
        doc_a.get_text("text").to_delta().to_json_value(),
        json!([
            {"insert": "A ", "attributes": {"bold": true}},
            {"insert": "fox", "attributes": {"bold": true, "italic": true}},
            {"insert": " jumped", "attributes": {"italic": true}}
        ])
    );
    for _ in 0..10 {
        undo.undo(&doc_a)?;
        assert_eq!(
            doc_a.get_text("text").to_delta().to_json_value(),
            json!([
                {"insert": "A " },
                {"insert": "fox jumped", "attributes": {"italic": true}}
            ])
        );
        // FIXME: right now redo/undo like this is wasteful
        undo.redo(&doc_a)?;
        assert_eq!(
            doc_a.get_text("text").to_delta().to_json_value(),
            json!([
                {"insert": "A ", "attributes": {"bold": true}},
                {"insert": "fox", "attributes": {"bold": true, "italic": true}},
                {"insert": " jumped", "attributes": {"italic": true}}
            ])
        );
    }

    Ok(())
}

#[test]
fn undo_richtext_conflict_set_style() -> LoroResult<()> {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1)?;
    let mut config = StyleConfigMap::new();
    config.insert(
        "color".into(),
        StyleConfig {
            expand: loro::ExpandType::After,
        },
    );
    doc_a.config_text_style(config.clone());
    let mut undo = UndoManager::new(&doc_a);
    let doc_b = LoroDoc::new();
    doc_b.config_text_style(config.clone());
    doc_b.set_peer_id(2)?;

    doc_a.get_text("text").insert(0, "A fox jumped")?;
    undo.record_new_checkpoint(&doc_a)?;
    sync(&doc_a, &doc_b);
    doc_b.get_text("text").mark(2..12, "color", "red")?;
    sync(&doc_a, &doc_b);
    doc_a.get_text("text").mark(0..5, "color", "green")?;
    undo.record_new_checkpoint(&doc_a)?;
    sync(&doc_a, &doc_b);
    assert_eq!(
        doc_a.get_text("text").to_delta().to_json_value(),
        json!([
            {"insert": "A fox", "attributes": {"color": "green"}},
            {"insert": " jumped", "attributes": {"color": "red"}}
        ])
    );
    for _ in 0..10 {
        undo.undo(&doc_a)?;
        assert_eq!(
            doc_a.get_text("text").to_delta().to_json_value(),
            json!([
                {"insert": "A " },
                {"insert": "fox jumped", "attributes": {"color": "red"}}
            ])
        );
        undo.undo(&doc_a)?;
        assert_eq!(doc_a.get_text("text").to_delta().to_json_value(), json!([]));
        undo.redo(&doc_a)?;
        assert_eq!(
            doc_a.get_text("text").to_delta().to_json_value(),
            json!([
                {"insert": "A " },
                {"insert": "fox jumped", "attributes": {"color": "red"}}
            ])
        );
        undo.redo(&doc_a)?;
        assert_eq!(
            doc_a.get_text("text").to_delta().to_json_value(),
            json!([
                {"insert": "A fox", "attributes": {"color": "green"}},
                {"insert": " jumped", "attributes": {"color": "red"}}
            ])
        );
    }

    Ok(())
}

#[test]
fn undo_text_collab_delete() -> LoroResult<()> {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1)?;
    let mut undo = UndoManager::new(&doc_a);
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2)?;
    doc_a.get_text("text").insert(0, "A ")?;
    undo.record_new_checkpoint(&doc_a)?;
    doc_a.get_text("text").insert(2, "fox ")?;
    undo.record_new_checkpoint(&doc_a)?;
    doc_a.get_text("text").insert(6, "jumped")?;
    undo.record_new_checkpoint(&doc_a)?;
    sync(&doc_a, &doc_b);

    doc_b.get_text("text").delete(2, 4)?;
    sync(&doc_a, &doc_b);
    doc_a.get_text("text").insert(0, "123!")?;
    undo.record_new_checkpoint(&doc_a)?;
    for _ in 0..3 {
        assert_eq!(doc_a.get_text("text").to_string(), "123!A jumped");
        undo.undo(&doc_a)?;
        assert_eq!(doc_a.get_text("text").to_string(), "A jumped");
        undo.undo(&doc_a)?;
        assert_eq!(doc_a.get_text("text").to_string(), "A ");
        undo.undo(&doc_a)?;
        assert_eq!(doc_a.get_text("text").to_string(), "");
        undo.redo(&doc_a)?;
        assert_eq!(doc_a.get_text("text").to_string(), "A ");
        undo.redo(&doc_a)?;
        assert_eq!(doc_a.get_text("text").to_string(), "A jumped");
        undo.redo(&doc_a)?;
        assert_eq!(doc_a.get_text("text").to_string(), "123!A jumped");
    }
    Ok(())
}

///
///                  ┌────────────┐        ┌────────────┐     ┌────────────┐
///                  │            │        │            │     │            │
///    Ops From B    │     A_     │◀──┬────│    fox     │ ◀─┬─│  _jumped.  │
///                  │            │   │    │            │   │ │            │
///                  └────────────┘   │    └────────────┘   │ └────────────┘
///                                   │                     │
///                                   │                     │
///                                   │                     │
///                  ┌────────────┐   │    ┌────────────┐   │  ┌────────────┐
///                  │            │   │    │    Make    │   │  │            │
///    Ops From A    │   Hello_   │◀──┴────│     "A"    │◀──┴──│    World   │
///                  │            │        │    bold    │      │            │
///                  └────────────┘        └────────────┘      └────────────┘
///     loop 3 {
///         A undo 3 times and redo 3 times
///     }
///     loop 3 {
///         B undo 3 times and redo 3 times
///     }
#[test]
fn collab_undo() -> anyhow::Result<()> {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1)?;
    let mut undo_a = UndoManager::new(&doc_a);
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2)?;
    let mut undo_b = UndoManager::new(&doc_b);

    doc_a.get_text("text").insert(0, "Hello ")?;
    doc_b.get_text("text").insert(0, "A ")?;
    sync(&doc_a, &doc_b);
    doc_b.get_text("text").insert(2 + 6, "fox")?;
    doc_a.get_text("text").mark(6..7, "bold", true)?;
    sync(&doc_a, &doc_b); // Hello A fox
    doc_b.get_text("text").insert(2 + 6 + 3, " jumped.")?;
    doc_a.get_text("text").insert(6, "World! ")?;
    sync(&doc_a, &doc_b); // Hello World! A fox jumped.

    for j in 0..3 {
        debug_span!("round A", j).in_scope(|| {
            assert!(!undo_a.can_redo(), "{:#?}", &undo_a);
            assert_eq!(
                doc_a.get_text("text").to_delta().to_json_value(),
                json!([
                    {"insert": "Hello World! "},
                    {"insert": "A", "attributes": {"bold": true}},
                    {"insert": " fox jumped."}
                ])
            );
            undo_a.undo(&doc_a)?;
            assert!(undo_a.can_redo());
            assert_eq!(
                doc_a.get_text("text").to_delta().to_json_value(),
                json!([
                    {"insert": "Hello "},
                    {"insert": "A", "attributes": {"bold": true}},
                    {"insert": " fox jumped."}
                ])
            );
            undo_a.undo(&doc_a)?;
            assert_eq!(
                doc_a.get_text("text").to_delta().to_json_value(),
                json!([
                    {"insert": "Hello A fox jumped."},
                ])
            );
            undo_a.undo(&doc_a)?;
            assert_eq!(
                doc_a.get_text("text").to_delta().to_json_value(),
                json!([
                    {"insert": "A fox jumped."},
                ])
            );

            assert!(!undo_a.can_undo());
            undo_a.redo(&doc_a)?;
            assert_eq!(
                doc_a.get_text("text").to_delta().to_json_value(),
                json!([
                    {"insert": "Hello A fox jumped."},
                ])
            );

            undo_a.redo(&doc_a)?;
            assert_eq!(
                doc_a.get_text("text").to_delta().to_json_value(),
                json!([
                    {"insert": "Hello "},
                    {"insert": "A", "attributes": {"bold": true}},
                    {"insert": " fox jumped."}
                ])
            );
            undo_a.redo(&doc_a)?;
            Ok::<(), LoroError>(())
        })?;
    }

    sync(&doc_a, &doc_b);
    for _ in 0..3 {
        assert!(!undo_b.can_redo());
        assert_eq!(
            doc_b.get_text("text").to_delta().to_json_value(),
            json!([
                {"insert": "Hello World! "},
                {"insert": "A", "attributes": {"bold": true}},
                {"insert": " fox jumped."}
            ])
        );
        undo_b.undo(&doc_b)?;
        assert!(undo_b.can_redo());
        assert_eq!(
            doc_b.get_text("text").to_delta().to_json_value(),
            json!([
                {"insert": "Hello World! "},
                {"insert": "A", "attributes": {"bold": true}},
                {"insert": " fox"}
            ])
        );

        undo_b.undo(&doc_b)?;
        assert_eq!(
            doc_b.get_text("text").to_delta().to_json_value(),
            json!([
                {"insert": "Hello World! "},
                {"insert": "A", "attributes": {"bold": true}},
                {"insert": " "},
            ])
        );
        undo_b.undo(&doc_b)?;
        assert_eq!(
            doc_b.get_text("text").to_delta().to_json_value(),
            json!([
                {"insert": "Hello World! "},
            ])
        );
        assert!(!undo_b.can_undo());
        assert!(undo_b.can_redo());
        undo_b.redo(&doc_b)?;
        assert_eq!(
            doc_b.get_text("text").to_delta().to_json_value(),
            json!([
                {"insert": "Hello World! "},
                {"insert": "A", "attributes": {"bold": true}},
                {"insert": " "},
            ])
        );
        undo_b.redo(&doc_b)?;
        assert_eq!(
            doc_b.get_text("text").to_delta().to_json_value(),
            json!([
                {"insert": "Hello World! "},
                {"insert": "A", "attributes": {"bold": true}},
                {"insert": " fox"}
            ])
        );
        undo_b.redo(&doc_b)?;
    }

    Ok(())
}

/// Undo/Redo this column
///
/// ┌───────┐
/// │  Map  │
/// └───────┘
///     ▲
///     │
/// ┌───────┐
/// │ List  │  1
/// └───────┘
///     ▲
///     │  "Hello World!"
/// ┌───────┐             ┌────────┐
/// │ Text  │◀─2──────────│ Remote │ "Fox"
/// └───────┘             │ Change │
///     ▲                 └────────┘
///     │                      ▲
/// ┌───────┐                  │
/// │ Text  │ " World!"        │
/// │ Edit  │ 3                │
/// └───────┘                  │
///     ▲                      │
///     │                      │
///     │     Mark bold        │
/// ┌───────┐ "Fox World!"     │
/// │ Text  │ 4                │
/// │ Edit  │──────────────────┘
/// └───────┘
///
#[test]
fn undo_sub_sub_container() -> anyhow::Result<()> {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1)?;
    let mut undo_a = UndoManager::new(&doc_a);
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2)?;
    let map_a = doc_a.get_map("map");
    let list_a = map_a.insert_container("list", LoroList::new())?;
    doc_a.commit();
    let text_a = list_a.insert_container(0, LoroText::new())?;
    doc_a.commit();
    text_a.insert(0, "Hello World!")?;
    sync(&doc_a, &doc_b);

    let text_b = doc_b.get_text(text_a.id());
    text_a.delete(0, 5)?;
    text_b.insert(0, "F")?;
    text_b.insert(2, "o")?;
    text_b.insert(4, "x")?;
    assert_eq!(
        text_b.to_delta().to_json_value(),
        json!([
            {"insert": "FHoexllo World!"},
        ])
    );
    sync(&doc_a, &doc_b);
    text_a.mark(0..3, "bold", true)?;
    assert_eq!(
        text_a.to_delta().to_json_value(),
        json!([
            {"insert": "Fox", "attributes": { "bold": true }},
            {"insert": " World!"}
        ])
    );

    undo_a.undo(&doc_a)?; // 4 -> 3
    assert_eq!(
        text_a.to_delta().to_json_value(),
        json!([
            {"insert": "Fox World!"},
        ])
    );
    undo_a.undo(&doc_a)?; // 3 -> 2
                          // It should be "FHoexllo World!" here ideally
                          // But it's too expensive to calculate and make the code too complicated
                          // So we skip the test
    undo_a.undo(&doc_a)?; // 2 -> 1.5
    assert_eq!(
        text_a.to_delta().to_json_value(),
        json!([
            {"insert": "Fox"},
        ])
    );
    undo_a.undo(&doc_a)?; // 1.5 -> 1
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({
            "map": {"list": []}
        })
    );

    undo_a.undo(&doc_a)?; // 1 -> 0
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({
            "map": {}
        })
    );

    undo_a.redo(&doc_a)?; // 0 -> 1
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({
            "map": {"list": []}
        })
    );
    undo_a.redo(&doc_a)?; // 1 -> 1.5
    assert_eq!(
        text_a.to_delta().to_json_value(),
        json!([
            {"insert": "Fox"},
        ])
    );
    undo_a.redo(&doc_a)?; // 1.5 -> 2

    undo_a.redo(&doc_a)?; // 2 -> 3

    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({
            "map": {
                "list": [
                    "Fox World!"
                ]
            }
        })
    );
    // there is a new text container, so we need to get it again
    let text_a = doc_a
        .get_by_str_path("map/list/0")
        .unwrap()
        .into_container()
        .unwrap()
        .into_text()
        .unwrap();
    undo_a.redo(&doc_a)?; // 3 -> 4
    assert_eq!(
        text_a.to_delta().to_json_value(),
        json!([
            {"insert": "Fox", "attributes": { "bold": true }},
            {"insert": " World!"}
        ])
    );

    Ok(())
}

/// ┌────┐
/// │"B" │
/// └────┘
///   ▲
/// ┌─┴───────┐
/// │bold "B" │
/// └─────────┘
///   ▲                Concurrent
///   │                  Delete
/// ┌─┴────────┐        ┌──────┐
/// │"Hello B" │◀───────│ "B"  │
/// └──────────┘        └──────┘
///   ▲                     ▲
///   │                     │
/// ┌─┴──┐                  │
/// │Undo│──────────────────┘
/// └────┘
///   ▲
///   │
/// ┌─┴──┐
/// │Undo│
/// └────┘
#[test]
fn test_remote_merge_transform() -> LoroResult<()> {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1)?;
    let mut undo_a = UndoManager::new(&doc_a);
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2)?;

    // Initial insert "B" in doc_a
    let text_a = doc_a.get_text("text");
    text_a.insert(0, "B")?;
    doc_a.commit();

    // Mark "B" as bold in doc_a
    text_a.mark(0..1, "bold", true)?;
    doc_a.commit();

    text_a.insert(0, "Hello ")?;
    doc_a.commit();

    // Sync doc_a to doc_b
    sync(&doc_a, &doc_b);

    // Concurrently delete "B" in doc_b
    let text_b = doc_b.get_text("text");
    text_b.delete(0, 6)?;
    sync(&doc_a, &doc_b);

    // Check the state after concurrent operations
    assert_eq!(
        text_a.to_delta().to_json_value(),
        json!([
            {"insert": "B", "attributes": {"bold": true}}
        ])
    );

    undo_a.undo(&doc_a)?;
    assert_eq!(
        text_a.to_delta().to_json_value(),
        json!([
            {"insert": "B"}
        ])
    );

    undo_a.undo(&doc_a)?;
    assert_eq!(text_a.to_delta().to_json_value(), json!([]));

    Ok(())
}

#[test]
fn undo_tree_move() -> LoroResult<()> {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1)?;
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2)?;
    let mut undo = UndoManager::new(&doc_a);
    let mut undo2 = UndoManager::new(&doc_b);
    let tree_a = doc_a.get_tree("tree");
    let tree_b = doc_b.get_tree("tree");
    let root = tree_a.create(None)?;
    let root2 = tree_b.create(None)?;
    doc_a.import(&doc_b.export_from(&Default::default()))?;
    doc_b.import(&doc_a.export_from(&Default::default()))?;
    tree_a.mov(root, root2)?;
    tree_b.mov(root2, root)?;
    doc_a.import(&doc_b.export_from(&Default::default()))?;
    doc_b.import(&doc_a.export_from(&Default::default()))?;
    let latest_value = tree_a.get_value();
    // a
    undo.undo(&doc_a)?;
    let a_value = tree_a.get_value().as_list().unwrap().clone();
    assert_eq!(a_value.len(), 2);
    assert!(a_value[0]
        .as_map()
        .unwrap()
        .get("parent")
        .unwrap()
        .is_null());
    assert!(a_value[1]
        .as_map()
        .unwrap()
        .get("parent")
        .unwrap()
        .is_null());

    undo.redo(&doc_a)?;
    assert_eq!(tree_a.get_value(), latest_value);
    // b
    undo2.undo(&doc_b)?;
    let b_value = tree_b.get_value().as_list().unwrap().clone();
    assert_eq!(b_value.len(), 0);
    undo.undo(&doc_a)?;
    undo.undo(&doc_a)?;
    let a_value = tree_a.get_value().as_list().unwrap().clone();
    assert_eq!(a_value.len(), 1);
    assert_eq!(
        a_value[0]
            .as_map()
            .unwrap()
            .get("id")
            .unwrap()
            .to_json_value(),
        json!("0@2")
    );
    assert!(a_value[0]
        .as_map()
        .unwrap()
        .get("parent")
        .unwrap()
        .is_null());
    Ok(())
}

#[test]
fn undo_tree_concurrent_delete() -> LoroResult<()> {
    let doc_a = LoroDoc::new();
    let tree_a = doc_a.get_tree("tree");
    let root = tree_a.create(None)?;
    let child = tree_a.create(Some(root))?;
    let doc_b = LoroDoc::new();
    let mut undo_b = UndoManager::new(&doc_b);
    let tree_b = doc_b.get_tree("tree");
    doc_b.import(&doc_a.export_from(&Default::default()))?;
    tree_a.delete(root)?;
    tree_b.delete(child)?;
    doc_a.import(&doc_b.export_from(&Default::default()))?;
    doc_b.import(&doc_a.export_from(&Default::default()))?;
    undo_b.undo(&doc_b)?;
    assert!(tree_b.get_value().as_list().unwrap().is_empty());
    Ok(())
}

#[test]
fn undo_tree_concurrent_delete2() -> LoroResult<()> {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1)?;
    let tree_a = doc_a.get_tree("tree");
    let root = tree_a.create(None)?;
    let child = tree_a.create(None)?;
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2)?;
    let mut undo_b = UndoManager::new(&doc_b);
    let tree_b = doc_b.get_tree("tree");
    doc_b.import(&doc_a.export_from(&Default::default()))?;
    tree_a.mov(child, root)?;
    tree_a.delete(root)?;
    tree_b.delete(child)?;
    doc_a.import(&doc_b.export_from(&Default::default()))?;
    doc_b.import(&doc_a.export_from(&Default::default()))?;
    undo_b.undo(&doc_b)?;
    assert_eq!(tree_b.get_value().as_list().unwrap().len(), 1);
    assert_eq!(
        tree_b.get_value().as_list().unwrap()[0]
            .as_map()
            .unwrap()
            .get("id")
            .unwrap()
            .to_json_value(),
        json!("1@1")
    );
    Ok(())
}

///                    ┌ ─ ─ ─ ─            ┌ ─ ─ ─ ─
///                      Peer 1 │             Peer 2 │
///                    └ ─ ─ ─ ─            └ ─ ─ ─ ─
///                    ┌────────┐
///                    │ Hello  │
///                    └────▲───┘
///                         │               ┌────────┐
///                    ┌────┴───┐           │ Delete │
///                    │ World  │◀──────────│ Hello  │
///                    └────▲───┘           └────▲───┘
///                         │                    │
///                    ┌────┴───┐           ┌────┴───┐
///                    │Insert 0│           │Insert  │
///                    │Alice   ◀────┐┌────▶│Hi      │
///                    └────▲───┘    ││     └────▲───┘
///                         │        ││          │
/// ┌ ─ ─ ─ ─ ─ ─ ─ ─  ┌────┴───┐    ││     ┌────┴───┐
///      Hi World    │ │  Undo  │────┼┘     │Delete  │
/// └ ─ ─ ─ ─ ─ ─ ─ ─  └────▲───┘    └──────│Alice   │
///                         │        ┌─────▶└────▲───┘
/// ┌ ─ ─ ─ ─ ─ ─ ─ ─  ┌────┴───┐    │           │
///         Hi       │ │  Undo  ├────┘      ┌────┴───┐
/// └ ─ ─ ─ ─ ─ ─ ─ ─  └────▲───┘           │Insert  │
///                         │        ┌─────▶│Bob     │
/// ┌ ─ ─ ─ ─ ─ ─ ─ ─  ┌────┴───┐    │      └────────┘
///       Bob Hi     │ │  Undo  ├────┘
/// └ ─ ─ ─ ─ ─ ─ ─ ─  └────▲───┘
///                         │
/// ┌ ─ ─ ─ ─ ─ ─ ─ ─  ┌────┴───┐
///      Bob Hi_     │ │  Redo  │
/// └ ─ ─ ─ ─ ─ ─ ─ ─  └────▲───┘
///                         │
/// ┌ ─ ─ ─ ─ ─ ─ ─ ─  ┌────┴───┐
///    Bob Hi World  │ │  Redo  │
/// └ ─ ─ ─ ─ ─ ─ ─ ─  └────▲───┘
///                         │
/// - ─ ─ ─ ─ ─ ─ ─ ─  ┌────┴───┐
/// Bob AliceHi World  │  Redo  │ It will reinsert "Alice" even if they were deleted by peer 2
/// - ─ ─ ─ ─ ─ ─ ─ ─  └────▲───┘
///                         │
///                    ┌ ─ ─ ─ ─
///                      Cannot │
///                    │  Redo
///                     ─ ─ ─ ─ ┘
#[test]
fn undo_redo_when_collab() -> anyhow::Result<()> {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1).unwrap();
    let mut undo_a = UndoManager::new(&doc_a);
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2).unwrap();

    let text_a = doc_a.get_text("text");
    text_a.insert(0, "Hello ")?;
    doc_a.commit();
    text_a.insert(6, "World")?;
    doc_a.commit();

    sync(&doc_a, &doc_b);

    let text_b = doc_b.get_text("text");
    text_b.delete(0, 5)?;
    doc_b.commit();
    text_b.insert(0, "Hi")?;
    doc_b.commit();

    text_a.insert(0, "Alice")?;
    sync(&doc_a, &doc_b);
    text_b.delete(0, 5)?;
    undo_a.undo(&doc_a)?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({
            "text": "Hi World"
        })
    );
    doc_a.import(&doc_b.export_from(&Default::default()))?;
    undo_a.undo(&doc_a)?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({
            "text": "Hi "
        })
    );
    text_b.insert(0, "Bob ")?;
    doc_a.import(&doc_b.export_from(&Default::default()))?;
    undo_a.undo(&doc_a)?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({
            "text": "Bob Hi"
        })
    );

    assert!(undo_a.can_redo());
    undo_a.redo(&doc_a)?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({
            "text": "Bob Hi "
        })
    );
    undo_a.redo(&doc_a)?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({
            "text": "Bob Hi World"
        })
    );
    undo_a.redo(&doc_a)?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({
            "text": "AliceBob Hi World"
        })
    );

    assert!(!undo_a.can_redo());

    Ok(())
}

#[test]
fn undo_list_move() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    let mut undo = UndoManager::new(&doc);
    list.insert(0, "0")?;
    doc.commit();
    list.insert(1, "1")?;
    doc.commit();
    list.insert(2, "2")?;
    doc.commit();

    list.mov(0, 2)?;
    doc.commit();
    list.mov(1, 0)?;
    doc.commit();
    for _ in 0..3 {
        assert!(!undo.can_redo());
        assert_eq!(
            doc.get_deep_value().to_json_value(),
            json!({
                "list": ["2", "1", "0"]
            })
        );
        undo.undo(&doc)?;
        assert!(undo.can_redo());
        assert_eq!(
            doc.get_deep_value().to_json_value(),
            json!({
                "list": ["1", "2", "0"]
            })
        );
        undo.undo(&doc)?;
        assert_eq!(
            doc.get_deep_value().to_json_value(),
            json!({
                "list": ["0", "1", "2"]
            })
        );
        undo.undo(&doc)?;
        assert_eq!(
            doc.get_deep_value().to_json_value(),
            json!({
                "list": ["0", "1"]
            })
        );

        undo.undo(&doc)?;
        undo.undo(&doc)?;
        assert!(!undo.can_undo());
        undo.redo(&doc)?;
        assert!(undo.can_undo());
        undo.redo(&doc)?;

        undo.redo(&doc)?;
        assert_eq!(
            doc.get_deep_value().to_json_value(),
            json!({
                "list": ["0", "1", "2"]
            })
        );
        undo.redo(&doc)?;
        assert_eq!(
            doc.get_deep_value().to_json_value(),
            json!({
                "list": ["1", "2", "0"]
            })
        );
        undo.redo(&doc)?;
        assert_eq!(
            doc.get_deep_value().to_json_value(),
            json!({
                "list": ["2", "1", "0"]
            })
        );
        assert!(!undo.can_redo());
    }
    Ok(())
}

#[ignore]
#[test]
fn undo_collab_list_move() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let list = doc.get_movable_list("list");
    list.insert(0, "0")?;
    list.insert(1, "1")?;
    list.insert(2, "2")?;
    doc.commit();
    let mut undo = UndoManager::new(&doc);
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2)?;
    doc_b.import(&doc.export_snapshot())?;
    list.mov(0, 2)?;
    assert_eq!(list.get_value().to_json_value(), json!(["1", "2", "0"]));
    doc.commit();
    doc_b.get_movable_list("list").mov(0, 1)?;
    sync(&doc, &doc_b);
    assert_eq!(list.get_value().to_json_value(), json!(["1", "0", "2"]));
    undo.undo(&doc)?;
    // FIXME: cannot infer move correctly for now
    assert_eq!(list.get_value().to_json_value(), json!(["0", "1", "2"]));
    Ok(())
}

#[test]
fn exclude_certain_local_ops_from_undo() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    undo.add_exclude_origin_prefix("sys:");
    doc.get_text("text").insert(0, "123")?;
    doc.commit();
    doc.get_text("text").insert(0, "x")?;
    doc.commit_with(CommitOptions::new().origin("sys:init"));
    doc.get_text("text").insert(2, "y")?; // x1y23
    doc.commit_with(CommitOptions::new().origin("sys:init"));
    doc.get_text("text").insert(4, "z")?; // x1y2z3
    doc.commit_with(CommitOptions::new().origin("sys:init"));
    doc.get_text("text").insert(6, "abc")?; // x1y2z3abc
    doc.commit();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "text": "x1y2z3abc"
        })
    );
    undo.undo(&doc)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "text": "x1y2z3"
        })
    );
    undo.undo(&doc)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "text": "xyz"
        })
    );
    assert!(!undo.can_undo());
    undo.redo(&doc)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "text": "x1y2z3"
        })
    );
    undo.redo(&doc)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "text": "x1y2z3abc"
        })
    );
    Ok(())
}

#[test]
fn should_not_trigger_update_when_undo_ops_depend_on_deleted_container() -> anyhow::Result<()> {
    let doc_a = LoroDoc::new();
    let doc_b = LoroDoc::new();
    let text = doc_a
        .get_map("map")
        .insert_container("text", LoroText::new())?;
    text.insert(0, "Hello")?;
    sync(&doc_a, &doc_b);
    let mut undo = UndoManager::new(&doc_b);
    let text_b = doc_b
        .get_by_str_path("map/text")
        .unwrap()
        .into_container()
        .unwrap()
        .into_text()
        .unwrap();
    text_b.insert(0, "Hi")?;
    doc_a.get_map("map").delete("text")?;
    sync(&doc_a, &doc_b);
    let f = doc_b.oplog_frontiers();
    undo.undo(&doc_b)?;
    // should not update doc_b, because the undo operation depends on a deleted container
    assert_eq!(f, doc_b.oplog_frontiers());
    Ok(())
}
