use anyhow::Result;
use loro::{ContainerTrait, ExportMode, LoroDoc, LoroText};

#[test]
fn new_keeps_public_auto_commit_behavior() -> Result<()> {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "hello")?;

    assert_eq!(text.to_string(), "hello");
    assert!(!doc.export(ExportMode::all_updates())?.is_empty());

    Ok(())
}

#[test]
fn from_snapshot_keeps_public_auto_commit_behavior() -> Result<()> {
    let doc = LoroDoc::new();
    doc.get_text("text").insert(0, "hello")?;
    let snapshot = doc.export(ExportMode::Snapshot)?;

    let restored = LoroDoc::from_snapshot(&snapshot)?;
    restored.get_text("text").insert(5, "!")?;

    assert_eq!(restored.get_text("text").to_string(), "hello!");

    let roundtrip = LoroDoc::from_snapshot(&restored.export(ExportMode::Snapshot)?)?;
    assert_eq!(roundtrip.get_deep_value(), restored.get_deep_value());

    Ok(())
}

#[test]
fn fork_at_keeps_public_auto_commit_behavior() -> Result<()> {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "hello")?;
    doc.commit();
    let first_frontiers = doc.state_frontiers();

    text.insert(5, " world")?;
    doc.commit();

    let forked = doc.fork_at(&first_frontiers);
    forked.get_text("text").insert(5, "!")?;

    assert_eq!(forked.get_text("text").to_string(), "hello!");

    let roundtrip = LoroDoc::from_snapshot(&forked.export(ExportMode::Snapshot)?)?;
    assert_eq!(roundtrip.get_deep_value(), forked.get_deep_value());

    Ok(())
}

#[test]
fn attached_container_doc_keeps_public_auto_commit_behavior() -> Result<()> {
    let doc = LoroDoc::new();
    let map = doc.get_map("map");
    let text = map.insert_container("text", LoroText::new())?;
    text.insert(0, "hello")?;

    let attached_doc = text.doc().expect("attached containers should expose their doc");
    attached_doc.get_text("other").insert(0, "world")?;

    assert_eq!(attached_doc.get_deep_value(), doc.get_deep_value());
    assert_eq!(doc.get_text("other").to_string(), "world");

    let roundtrip = LoroDoc::from_snapshot(&attached_doc.export(ExportMode::Snapshot)?)?;
    assert_eq!(roundtrip.get_deep_value(), attached_doc.get_deep_value());

    Ok(())
}
