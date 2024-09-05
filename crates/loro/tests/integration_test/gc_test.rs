use super::gen_action;
use loro::LoroDoc;

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
