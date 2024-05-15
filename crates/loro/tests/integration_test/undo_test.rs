use loro::{Frontiers, LoroDoc, LoroError, ToJson};
use loro_internal::{
    id::{Counter, ID},
    loro_common::IdSpan,
};
use serde_json::json;

#[test]
fn basic_text_undo() -> Result<(), LoroError> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let text = doc.get_text("text");
    text.insert(0, "123")?;
    doc.commit();
    doc.undo(ID::new(1, 1).into())?;
    assert_eq!(doc.get_deep_value().to_json_value(), json!({"text": "13"}));
    assert_eq!(doc.oplog_frontiers(), Frontiers::from(ID::new(1, 3)));
    assert_eq!(doc.state_frontiers(), Frontiers::from(ID::new(1, 3)));

    // This should not change anything, because the content is already deleted
    doc.undo(ID::new(1, 1).into())?;
    assert_eq!(doc.get_deep_value().to_json_value(), json!({"text": "13"}));
    assert_eq!(doc.oplog_frontiers(), Frontiers::from(ID::new(1, 3)));
    assert_eq!(doc.state_frontiers(), Frontiers::from(ID::new(1, 3)));

    // This should remove the content
    doc.undo(IdSpan::new(1, 0, 3))?;
    assert_eq!(doc.get_deep_value().to_json_value(), json!({"text": ""}));
    assert_eq!(doc.oplog_frontiers(), Frontiers::from(ID::new(1, 5)));
    assert_eq!(doc.state_frontiers(), Frontiers::from(ID::new(1, 5)));

    // Now we redo the undos
    doc.undo(IdSpan::new(1, 3, 6))?;
    assert_eq!(doc.get_deep_value().to_json_value(), json!({"text": "123"}));
    assert_eq!(doc.oplog_frontiers(), Frontiers::from(ID::new(1, 8)));
    assert_eq!(doc.state_frontiers(), Frontiers::from(ID::new(1, 8)));
    Ok(())
}

#[test]
fn text_undo_insert_should_only_delete_once() -> Result<(), LoroError> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let text = doc.get_text("text");
    text.insert(0, "123")?;
    doc.commit();
    text.delete(1, 2)?;
    doc.commit();
    assert_eq!(doc.get_deep_value().to_json_value(), json!({"text": "1"}));

    // nothing should happen here, because the delete has already happened
    doc.undo(ID::new(1, 1).into())?;
    assert_eq!(doc.get_deep_value().to_json_value(), json!({"text": "1"}));

    // nothing should happen here, because the delete has already happened
    doc.undo(ID::new(1, 2).into())?;
    assert_eq!(doc.get_deep_value().to_json_value(), json!({"text": "1"}));

    doc.undo(ID::new(1, 0).into())?;
    assert_eq!(doc.get_deep_value().to_json_value(), json!({"text": ""}));
    Ok(())
}

#[test]
fn collaborative_text_undo() -> Result<(), LoroError> {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1)?;
    let text = doc_a.get_text("text");
    text.insert(0, "123")?;
    doc_a.commit();

    let doc_b = LoroDoc::new();
    doc_b.import(&doc_a.export_from(&Default::default()))?;
    doc_b.get_text("text").insert(1, "y")?;
    doc_b.commit();
    doc_b.get_text("text").insert(0, "x")?;
    // doc_b = x1y23
    doc_b.commit();
    // doc_a = x1y23
    doc_a.import(&doc_b.export_from(&Default::default()))?;

    doc_a.undo(ID::new(1, 0).into())?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({"text": "xy23"})
    );
    assert_eq!(doc_a.oplog_frontiers(), Frontiers::from(ID::new(1, 3)));
    assert_eq!(doc_a.state_frontiers(), Frontiers::from(ID::new(1, 3)));

    // This should not change anything, because the content is already deleted
    doc_a.undo(ID::new(1, 0).into())?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({"text": "xy23"})
    );
    assert_eq!(doc_a.oplog_frontiers(), Frontiers::from(ID::new(1, 3)));
    assert_eq!(doc_a.state_frontiers(), Frontiers::from(ID::new(1, 3)));

    // This should remove the content created by A
    doc_a.undo(IdSpan::new(1, 0, 3))?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({"text": "xy"})
    );
    assert_eq!(doc_a.oplog_frontiers(), Frontiers::from(ID::new(1, 5)));
    assert_eq!(doc_a.state_frontiers(), Frontiers::from(ID::new(1, 5)));

    // Now we redo the undos
    doc_a.undo(IdSpan::new(1, 3, 6))?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({"text": "x1y23"})
    );
    assert_eq!(doc_a.oplog_frontiers(), Frontiers::from(ID::new(1, 8)));
    assert_eq!(doc_a.state_frontiers(), Frontiers::from(ID::new(1, 8)));
    Ok(())
}

#[test]
fn basic_list_undo_insertion() -> Result<(), LoroError> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let list = doc.get_list("list");
    list.push("12")?;
    list.push("34")?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "list": ["12", "34"]
        })
    );
    doc.undo(ID::new(1, 1).into())?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "list": ["12"]
        })
    );
    doc.undo(ID::new(1, 0).into())?;
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
    list.push("12")?; // op 0
    list.push("34")?; // op 1
    list.delete(1, 1)?; // op 2
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "list": ["12"]
        })
    );
    doc.undo(ID::new(1, 2).into())?; // op 3
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "list": ["12", "34"]
        })
    );

    // Now, to undo "34" correctly we need to include the latest change
    // If we only undo op 1, op 3 will create "34" again.
    doc.undo(IdSpan::new(1, 1, 4))?; // op 4
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
    doc_a.get_map("map").insert("a", "a")?;
    doc_a.get_map("map").insert("b", "b")?;
    doc_a.commit();
    doc_a.get_map("map").delete("a")?;
    doc_a.commit();
    doc_a.undo(ID::new(1, 2).into())?; // op 3
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({"map": {"a": "a", "b": "b"}})
    );

    doc_a.undo(ID::new(1, 1).into())?; // op 4
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({"map": {"a": "a"}})
    );

    doc_a.undo(ID::new(1, 0).into())?; // op 5
    assert_eq!(doc_a.get_deep_value().to_json_value(), json!({"map": {}}));

    // Redo
    doc_a.undo(ID::new(1, 5).into())?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({"map": {
            "a": "a"
        }})
    );

    // Redo
    doc_a.undo(ID::new(1, 4).into())?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({"map": {
            "a": "a",
            "b": "b"
        }})
    );

    // Redo
    doc_a.undo(ID::new(1, 3).into())?;
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
    doc_a.get_map("map").insert("a", "a")?;
    doc_a.commit();

    let doc_b = LoroDoc::new();
    doc_b.import(&doc_a.export_from(&Default::default()))?;
    doc_b.get_map("map").insert("b", "b")?;
    doc_b.commit();

    doc_a.import(&doc_b.export_from(&Default::default()))?;
    doc_a.undo(ID::new(1, 0).into())?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({"map": {"b": "b"}})
    );
    Ok(())
}

#[test]
fn map_container_undo() -> Result<(), LoroError> {
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
    doc_a.undo(IdSpan::new(1, 0, 8))?;
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
    doc_a.undo(IdSpan::new(1, 0, (TIMES * 8) as Counter))?;
    assert_eq!(
        doc_a.get_deep_value().to_json_value(),
        json!({
            "text": "B".repeat(TIMES )
        })
    );
    Ok(())
}
