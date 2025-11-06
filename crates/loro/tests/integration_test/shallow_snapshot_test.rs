use std::{
    borrow::Cow,
    sync::{atomic::AtomicBool, Arc},
};

use super::gen_action;
use loro::{cursor::CannotFindRelativePosition, ExportMode, Frontiers, LoroDoc, ID};

#[test]
fn test_gc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 32);
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    gen_action(&doc, 123, 10);
    doc.commit();
    let shallow_bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers));

    let new_doc = LoroDoc::new();
    new_doc.import(&shallow_bytes.unwrap())?;
    assert_eq!(doc.get_deep_value(), new_doc.get_deep_value());
    Ok(())
}

#[test]
fn test_shallow_1() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "1")?;
    doc.get_text("text").insert(0, "2")?;
    doc.get_text("text").insert(0, "3")?;
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    doc.get_text("text").insert(3, "4")?;
    doc.commit();
    let shallow_bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers));

    let new_doc = LoroDoc::new();
    new_doc.import(&shallow_bytes.unwrap())?;
    assert_eq!(doc.get_deep_value(), new_doc.get_deep_value());
    Ok(())
}

#[test]
fn test_checkout_to_text_that_were_created_before_gc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "0")?;
    doc.get_text("text").insert(0, "1")?;
    doc.get_text("text").insert(0, "2")?;
    doc.get_text("text").insert(1, "3")?;
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    doc.get_text("text").delete(0, 3)?;
    let bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap())?;
    new_doc.checkout(&frontiers)?;
    assert_eq!(new_doc.get_text("text").to_string(), *"2310");
    Ok(())
}

#[test]
fn test_checkout_to_list_that_were_created_before_gc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_list("list").insert(0, 0)?;
    doc.get_list("list").insert(1, 1)?;
    doc.get_list("list").insert(2, 2)?;
    doc.get_list("list").insert(1, 3)?;
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    doc.get_list("list").delete(0, 3)?;
    let bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap())?;
    new_doc.checkout(&frontiers)?;
    assert_eq!(
        new_doc.get_list("list").to_vec(),
        vec![0.into(), 3.into(), 1.into(), 2.into()]
    );
    Ok(())
}

#[test]
fn test_checkout_to_movable_list_that_were_created_before_gc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_movable_list("list").insert(0, 0)?;
    doc.get_movable_list("list").insert(1, 1)?;
    doc.get_movable_list("list").insert(2, 2)?;
    doc.get_movable_list("list").insert(1, 3)?;
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    doc.get_movable_list("list").delete(0, 3)?;
    let bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap())?;
    new_doc.checkout(&frontiers)?;
    assert_eq!(
        new_doc.get_movable_list("list").to_vec(),
        vec![0.into(), 3.into(), 1.into(), 2.into()]
    );
    Ok(())
}

#[test]
fn shallow_on_the_given_version_when_feasible() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 64);
    doc.commit();
    let bytes = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(1, 31)));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap())?;
    assert_eq!(new_doc.shallow_since_vv().get(&1).copied().unwrap(), 31);
    Ok(())
}

#[test]
fn export_snapshot_on_a_shallow_doc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 32);
    doc.commit();

    // Get the current frontiers
    let frontiers = doc.oplog_frontiers();
    let old_value = doc.get_deep_value();
    gen_action(&doc, 123, 32);
    doc.commit();

    // Export using shallowSnapshot mode
    let bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers));

    // Import into a new document
    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&bytes.unwrap())?;
    assert_eq!(shallow_doc.shallow_since_vv().get(&1).copied().unwrap(), 31);
    let new_snapshot = shallow_doc.export(loro::ExportMode::Snapshot);

    let new_doc = LoroDoc::new();
    new_doc.import(&new_snapshot.unwrap())?;
    assert_eq!(new_doc.shallow_since_vv().get(&1).copied().unwrap(), 31);
    assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());
    new_doc.checkout(&frontiers)?;
    assert_eq!(new_doc.get_deep_value(), old_value);
    new_doc.checkout_to_latest();
    assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());
    Ok(())
}

#[test]
fn test_richtext_gc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let text = doc.get_text("text");
    text.insert(0, "1")?; // 0
    text.insert(0, "2")?; // 1
    text.insert(0, "3")?; // 2
    text.mark(0..2, "bold", "value")?; // 3, 4
    doc.commit();
    text.insert(3, "456")?; // 5, 6, 7
    let bytes = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(1, 3)));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap())?;
    new_doc.checkout(&Frontiers::from(ID::new(1, 4)))?;
    assert_eq!(new_doc.get_text("text").to_string(), "321");
    new_doc.checkout_to_latest();
    assert_eq!(new_doc.get_text("text").to_string(), "321456");
    Ok(())
}

#[test]
fn import_updates_depend_on_shallow_history_should_raise_error() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 4);
    doc.commit();
    let doc2 = doc.fork();
    doc2.get_text("text").insert(0, "1")?;
    doc2.commit();
    gen_action(&doc, 123, 2);
    doc.commit();
    let shallow_snapshot = doc.export(loro::ExportMode::shallow_snapshot(&doc.oplog_frontiers()));
    doc.get_text("hello").insert(0, "world").unwrap();
    doc2.import(
        &doc.export(loro::ExportMode::Updates {
            from: Cow::Borrowed(&doc2.oplog_vv()),
        })
        .unwrap(),
    )
    .unwrap();

    let new_doc = LoroDoc::new();
    new_doc.import(&shallow_snapshot.unwrap()).unwrap();

    let ran = Arc::new(AtomicBool::new(false));
    let ran_clone = ran.clone();
    let _sub = new_doc.subscribe_root(Arc::new(move |e| {
        ran_clone.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(e.events.len() == 1);
        match e.events[0].diff {
            loro::event::Diff::Text(_) => {}
            _ => {
                unreachable!()
            }
        }
    }));
    let result = new_doc.import(
        &doc2
            .export(loro::ExportMode::updates_owned(new_doc.oplog_vv()))
            .unwrap(),
    );
    assert!(result.is_err());
    // But updates from doc should be fine ("hello": "world")
    assert_eq!(new_doc.get_text("hello").to_string(), *"world");
    assert!(ran.load(std::sync::atomic::Ordering::Relaxed));
    Ok(())
}

#[test]
fn the_vv_on_shallow_doc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    gen_action(&doc, 0, 10);
    doc.commit();
    let snapshot = doc.export(loro::ExportMode::shallow_snapshot(&doc.oplog_frontiers()));
    let new_doc = LoroDoc::new();
    new_doc.import(&snapshot.unwrap()).unwrap();
    assert!(!new_doc.shallow_since_vv().is_empty());
    assert_eq!(new_doc.oplog_vv(), new_doc.state_vv());
    assert_eq!(new_doc.oplog_vv(), doc.state_vv());
    assert_eq!(new_doc.oplog_frontiers(), doc.oplog_frontiers());
    assert_eq!(new_doc.oplog_frontiers(), new_doc.state_frontiers());
    assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());

    gen_action(&doc, 0, 10);
    doc.commit();
    let bytes = doc.export(ExportMode::all_updates());
    new_doc.import(&bytes.unwrap()).unwrap();
    assert_eq!(new_doc.oplog_vv(), new_doc.state_vv());
    assert_eq!(new_doc.oplog_vv(), doc.state_vv());
    assert_eq!(new_doc.oplog_frontiers(), doc.oplog_frontiers());
    assert_eq!(new_doc.oplog_frontiers(), new_doc.state_frontiers());
    assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());

    Ok(())
}

#[test]
fn no_event_when_exporting_shallow_snapshot() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 0, 10);
    doc.commit();
    let _id = doc.subscribe_root(Arc::new(|_diff| {
        panic!("should not emit event");
    }));
    let _snapshot = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(1, 3)));
    Ok(())
}

#[test]
fn test_cursor_that_cannot_be_found_when_exporting_shallow_snapshot() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "Hello world")?;
    let c = doc
        .get_text("text")
        .get_cursor(3, loro::cursor::Side::Left)
        .unwrap();
    doc.get_text("text").delete(0, 5)?;
    doc.commit();
    let snapshot = doc.export(loro::ExportMode::shallow_snapshot(&doc.oplog_frontiers()));
    let new_doc = LoroDoc::new();
    new_doc.import(&snapshot.unwrap())?;
    let result = new_doc.get_cursor_pos(&c);
    match result {
        Ok(v) => {
            dbg!(v);
            unreachable!()
        }
        Err(CannotFindRelativePosition::HistoryCleared) => {}
        Err(x) => {
            dbg!(x);
            unreachable!()
        }
    }
    Ok(())
}

#[test]
fn test_cursor_that_can_be_found_when_exporting_shallow_snapshot() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "Hello world")?;
    doc.commit();
    let c = doc
        .get_text("text")
        .get_cursor(3, loro::cursor::Side::Left)
        .unwrap();
    doc.get_text("text").delete(0, 5)?;
    doc.commit();
    let snapshot = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(1, 10)));
    let new_doc = LoroDoc::new();
    new_doc.import(&snapshot.unwrap())?;
    let result = new_doc.get_cursor_pos(&c);
    match result {
        Ok(v) => {
            assert_eq!(v.current.pos, 0);
        }
        Err(x) => {
            dbg!(x);
            unreachable!()
        }
    }
    Ok(())
}

#[test]
fn test_export_shallow_snapshot_from_shallow_doc() -> anyhow::Result<()> {
    // Create and populate the original document
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 32);
    doc.commit();

    // Get the current frontiers and create some more actions
    let frontiers = doc.oplog_frontiers();
    gen_action(&doc, 123, 32);
    doc.commit();

    // Export using shallowSnapshot mode
    let shallow_bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers))?;

    // Import into a new document
    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&shallow_bytes)?;

    // Attempt to export a shallow snapshot from the shallow document
    // using frontiers before its shallow version
    let result = shallow_doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(1, 16)));

    // The export should fail because the requested frontiers are before the shallow version
    assert!(result.is_err());

    if let Err(e) = result {
        assert!(matches!(e, loro::LoroEncodeError::FrontiersNotFound(..)));
    } else {
        panic!("Expected an error, but got Ok");
    }

    Ok(())
}

#[test]
#[should_panic]
fn test_export_snapshot_from_shallow_doc() {
    // Create and populate the original document
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    gen_action(&doc, 123, 32);
    doc.commit();

    // Get the current frontiers and create some more actions
    let frontiers = doc.oplog_frontiers();
    gen_action(&doc, 123, 32);
    doc.commit();

    // Export using shallowSnapshot mode
    let shallow_bytes = doc
        .export(loro::ExportMode::shallow_snapshot(&frontiers))
        .unwrap();

    // Import into a new document
    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&shallow_bytes).unwrap();

    // Attempt to export a shallow snapshot from the shallow document
    // using frontiers before its shallow version
    shallow_doc.export(loro::ExportMode::Snapshot).unwrap();
}
