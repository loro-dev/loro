use std::sync::{atomic::AtomicBool, Arc};

use super::gen_action;
use loro::{Frontiers, LoroDoc, ID};

#[test]
fn test_gc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 32);
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    gen_action(&doc, 123, 10);
    doc.commit();
    let gc_bytes = doc.export(loro::ExportMode::GcSnapshot(&frontiers));

    let new_doc = LoroDoc::new();
    new_doc.import(&gc_bytes)?;
    assert_eq!(doc.get_deep_value(), new_doc.get_deep_value());
    Ok(())
}

#[test]
fn test_gc_1() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "1")?;
    doc.get_text("text").insert(0, "2")?;
    doc.get_text("text").insert(0, "3")?;
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    doc.get_text("text").insert(3, "4")?;
    doc.commit();
    let gc_bytes = doc.export(loro::ExportMode::GcSnapshot(&frontiers));

    let new_doc = LoroDoc::new();
    new_doc.import(&gc_bytes)?;
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
    let bytes = doc.export(loro::ExportMode::GcSnapshot(&frontiers));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes)?;
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
    let bytes = doc.export(loro::ExportMode::GcSnapshot(&frontiers));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes)?;
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
    let bytes = doc.export(loro::ExportMode::GcSnapshot(&frontiers));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes)?;
    new_doc.checkout(&frontiers)?;
    assert_eq!(
        new_doc.get_movable_list("list").to_vec(),
        vec![0.into(), 3.into(), 1.into(), 2.into()]
    );
    Ok(())
}

#[test]
fn gc_on_the_given_version_when_feasible() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 64);
    doc.commit();
    let bytes = doc.export(loro::ExportMode::GcSnapshot(&Frontiers::from(ID::new(
        1, 31,
    ))));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes)?;
    assert_eq!(new_doc.trimmed_vv().get(&1).copied().unwrap(), 31);
    Ok(())
}

#[test]
fn export_snapshot_on_a_trimmed_doc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 32);
    doc.commit();

    // Get the current frontiers
    let frontiers = doc.oplog_frontiers();
    let old_value = doc.get_deep_value();
    gen_action(&doc, 123, 32);
    doc.commit();

    // Export using GcSnapshot mode
    let bytes = doc.export(loro::ExportMode::GcSnapshot(&frontiers));

    // Import into a new document
    let trimmed_doc = LoroDoc::new();
    trimmed_doc.import(&bytes)?;
    assert_eq!(trimmed_doc.trimmed_vv().get(&1).copied().unwrap(), 31);
    let new_snapshot = trimmed_doc.export(loro::ExportMode::Snapshot);

    let new_doc = LoroDoc::new();
    new_doc.import(&new_snapshot)?;
    assert_eq!(new_doc.trimmed_vv().get(&1).copied().unwrap(), 31);
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
    let bytes = doc.export(loro::ExportMode::GcSnapshot(&Frontiers::from(ID::new(
        1, 3,
    ))));

    let new_doc = LoroDoc::new();
    new_doc.import(&bytes)?;
    new_doc.checkout(&Frontiers::from(ID::new(1, 4)))?;
    assert_eq!(new_doc.get_text("text").to_string(), "321");
    new_doc.checkout_to_latest();
    assert_eq!(new_doc.get_text("text").to_string(), "321456");
    Ok(())
}

#[test]
fn import_updates_depend_on_trimmed_history_should_raise_error() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 4);
    doc.commit();
    let doc2 = doc.fork();
    doc2.get_text("text").insert(0, "1")?;
    doc2.commit();
    gen_action(&doc, 123, 2);
    doc.commit();
    let gc_snapshot = doc.export(loro::ExportMode::GcSnapshot(&doc.oplog_frontiers()));
    doc.get_text("hello").insert(0, "world").unwrap();
    doc2.import(&doc.export(loro::ExportMode::Updates {
        from: &doc2.oplog_vv(),
    }))
    .unwrap();

    let new_doc = LoroDoc::new();
    new_doc.import(&gc_snapshot).unwrap();

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
    let result = new_doc.import(&doc2.export(loro::ExportMode::Updates {
        from: &new_doc.oplog_vv(),
    }));
    assert!(result.is_err());
    // But updates from doc should be fine ("hello": "world")
    assert_eq!(new_doc.get_text("hello").to_string(), *"world");
    assert!(ran.load(std::sync::atomic::Ordering::Relaxed));
    Ok(())
}
