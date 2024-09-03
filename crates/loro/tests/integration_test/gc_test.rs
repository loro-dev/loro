use super::gen_action;
use loro::LoroDoc;

#[test]
fn test_gc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 32);
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    dbg!(&frontiers);
    gen_action(&doc, 123, 10);
    doc.commit();
    dbg!(doc.oplog_frontiers());
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
    dbg!(&frontiers);
    doc.get_text("text").insert(3, "4")?;
    doc.commit();
    dbg!(doc.oplog_frontiers());
    let gc_bytes = doc.export(loro::ExportMode::GcSnapshot(&frontiers));

    let new_doc = LoroDoc::new();
    new_doc.import(&gc_bytes)?;
    assert_eq!(doc.get_deep_value(), new_doc.get_deep_value());
    Ok(())
}
