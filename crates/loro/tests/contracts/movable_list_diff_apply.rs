use loro::{ExportMode, LoroDoc, LoroMap, LoroResult, LoroText, ToJson};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

#[test]
fn movable_list_diff_apply_preserves_moves_container_replacements_and_reverse_patch(
) -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(91)?;

    let list = doc.get_movable_list("queue");
    list.push("alpha")?;
    let first_map = list.push_container(LoroMap::new())?;
    first_map.insert("id", "first")?;
    first_map.insert("rank", 1)?;
    let second_text = list.push_container(LoroText::new())?;
    second_text.insert(0, "second")?;
    list.push("tail")?;
    doc.commit();

    let v1 = doc.state_frontiers();
    let snapshot_v1 = doc.export(ExportMode::Snapshot)?;
    let value_v1 = deep_json(&doc);
    assert_eq!(
        value_v1,
        json!({
            "queue": ["alpha", {"id": "first", "rank": 1}, "second", "tail"]
        })
    );

    list.mov(2, 0)?;
    let replacement = list.insert_container(1, LoroMap::new())?;
    replacement.insert("id", "replacement")?;
    replacement.insert("rank", 2)?;
    list.delete(3, 1)?;
    list.set(3, "tail-updated")?;
    doc.commit();

    let v2 = doc.state_frontiers();
    let snapshot_v2 = doc.export(ExportMode::Snapshot)?;
    let value_v2 = deep_json(&doc);
    assert_eq!(
        value_v2,
        json!({
            "queue": ["second", {"id": "replacement", "rank": 2}, "alpha", "tail-updated"]
        })
    );

    let forward = doc.diff(&v1, &v2)?;
    let patched = LoroDoc::from_snapshot(&snapshot_v1)?;
    patched.apply_diff(forward)?;
    assert_eq!(deep_json(&patched), value_v2);

    let reverse = doc.diff(&v2, &v1)?;
    let restored = LoroDoc::from_snapshot(&snapshot_v2)?;
    restored.apply_diff(reverse)?;
    assert_eq!(deep_json(&restored), value_v1);

    Ok(())
}
