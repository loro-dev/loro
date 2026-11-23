//! group_start opens a new undo item even inside the merge interval.

use loro::{ExportMode, LoroDoc, LoroResult, ToJson, UndoManager};
use serde_json::json;

fn peer(id: u64) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(id).unwrap();
    doc
}

fn send(from: &LoroDoc, to: &LoroDoc) -> LoroResult<()> {
    from.commit();
    to.import(&from.export(ExportMode::all_updates())?)?;
    Ok(())
}

#[test]
fn group_start_does_not_merge_into_the_previous_item() -> LoroResult<()> {
    let a = peer(1);
    let list = a.get_movable_list("list");
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(i64::MAX);

    list.push(10)?;
    a.commit();
    undo.group_start()?;
    list.push(20)?;
    a.commit();
    undo.group_end();

    assert!(undo.undo()?);
    assert_eq!(list.get_value().to_json_value(), json!([10]));
    assert!(undo.can_undo());
    Ok(())
}

#[test]
fn group_start_does_not_merge_after_a_disjoint_remote_edit() -> LoroResult<()> {
    let a = peer(1);
    let b = peer(2);
    a.get_text("text").insert(0, "x")?;
    send(&a, &b)?;

    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(i64::MAX);
    let list = a.get_movable_list("list");
    list.push(10)?;
    a.commit();

    undo.group_start()?;
    b.get_text("text").mark(0..1, "bold", true)?;
    send(&b, &a)?;
    let remote = b.get_text("text").get_richtext_value();
    list.push(20)?;
    a.commit();
    undo.group_end();

    assert!(undo.undo()?);
    send(&a, &b)?;
    assert_eq!(list.get_value().to_json_value(), json!([10]));
    assert_eq!(
        b.get_movable_list("list").get_value().to_json_value(),
        json!([10])
    );
    assert_eq!(a.get_text("text").get_richtext_value(), remote);
    assert!(undo.can_undo());
    Ok(())
}
