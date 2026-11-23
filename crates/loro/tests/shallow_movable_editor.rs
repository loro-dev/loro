//! A shallow snapshot keeps who wrote a movable-list value apart from who moved it.

use loro::{ExportMode, LoroDoc, LoroResult, ToJson};
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
fn shallow_checkout_keeps_editor_distinct_from_mover() -> LoroResult<()> {
    let a = peer(1);
    let b = peer(2);
    let list = a.get_movable_list("list");
    list.push("initial")?;
    list.push("tail")?;
    send(&a, &b)?;

    b.get_movable_list("list").set(0, "written-by-b")?;
    send(&b, &a)?;
    list.mov(0, 1)?;
    a.commit();
    let root = a.state_frontiers();
    assert_eq!(list.get_last_editor_at(1), Some(2));
    assert_eq!(list.get_last_mover_at(1), Some(1));

    send(&a, &b)?;
    b.get_movable_list("list").set(1, "later")?;
    send(&b, &a)?;

    let full = LoroDoc::from_snapshot(&a.export(ExportMode::Snapshot)?)?;
    let shallow = LoroDoc::from_snapshot(&a.export(ExportMode::shallow_snapshot(&root))?)?;
    full.checkout(&root)?;
    shallow.checkout(&root)?;
    let full_list = full.get_movable_list("list");
    let shallow_list = shallow.get_movable_list("list");

    assert_eq!(
        shallow_list.get_value().to_json_value(),
        json!(["tail", "written-by-b"])
    );
    assert_eq!(shallow_list.get_value(), full_list.get_value());
    assert_eq!(full_list.get_last_editor_at(1), Some(2));
    assert_eq!(shallow_list.get_last_mover_at(1), Some(1));
    assert_eq!(shallow_list.get_last_editor_at(1), Some(2));
    Ok(())
}
