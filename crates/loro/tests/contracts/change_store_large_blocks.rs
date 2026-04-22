use loro::{CommitOptions, ExportMode, IdSpan, LoroDoc, LoroResult, ToJson, ID};
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn compacted_large_change_store_keeps_metadata_ranges_and_import_contracts() -> LoroResult<()> {
    const CHANGE_NUM: usize = 1_100;

    let doc = LoroDoc::new();
    doc.set_peer_id(801)?;
    doc.set_change_merge_interval(0);
    doc.set_record_timestamp(true);

    let text = doc.get_text("text");
    for i in 0..CHANGE_NUM {
        text.insert(text.len_unicode(), "x")?;
        let _ = doc.commit_with(
            CommitOptions::new()
                .timestamp((i + 1) as i64)
                .commit_msg(&format!("change-{i}")),
        );
    }

    assert_eq!(doc.len_changes(), CHANGE_NUM);
    let expected = doc.get_deep_value().to_json_value();
    assert_eq!(expected, json!({ "text": "x".repeat(CHANGE_NUM) }));

    doc.compact_change_store();
    assert_eq!(doc.len_changes(), CHANGE_NUM);
    for index in [0, CHANGE_NUM / 2, CHANGE_NUM - 1] {
        let change = doc
            .get_change(ID::new(801, index as i32))
            .expect("compacted change metadata should remain addressable");
        assert_eq!(change.message(), format!("change-{index}"));
        assert_eq!(change.timestamp(), (index + 1) as i64);
    }

    let all_updates = doc.export(ExportMode::all_updates())?;
    let all_meta = LoroDoc::decode_import_blob_meta(&all_updates, false)?;
    assert_eq!(all_meta.change_num, CHANGE_NUM as u32);
    assert_eq!(all_meta.partial_start_vv.get(&801).copied().unwrap_or(0), 0);
    assert_eq!(
        all_meta.partial_end_vv.get(&801).copied(),
        Some(CHANGE_NUM as i32)
    );

    let imported = LoroDoc::new();
    imported.import(&all_updates)?;
    assert_eq!(imported.get_deep_value().to_json_value(), expected);
    assert_eq!(
        imported
            .get_change(ID::new(801, (CHANGE_NUM / 2) as i32))
            .unwrap()
            .message(),
        format!("change-{}", CHANGE_NUM / 2)
    );

    let first = doc.export(ExportMode::updates_in_range(vec![IdSpan::new(801, 0, 400)]))?;
    let middle = doc.export(ExportMode::updates_in_range(vec![IdSpan::new(
        801, 400, 800,
    )]))?;
    let last = doc.export(ExportMode::updates_in_range(vec![IdSpan::new(
        801,
        800,
        CHANGE_NUM as i32,
    )]))?;
    assert_eq!(
        LoroDoc::decode_import_blob_meta(&middle, false)?
            .partial_start_vv
            .get(&801)
            .copied(),
        Some(400)
    );

    let replay = LoroDoc::new();
    let pending = replay.import_batch(&[last.clone()])?;
    assert_eq!(
        pending.pending.as_ref().and_then(|p| p.get(&801).copied()),
        Some((800, CHANGE_NUM as i32))
    );
    let complete = replay.import_batch(&[middle, first])?;
    assert!(complete.pending.is_none());
    assert_eq!(replay.get_deep_value().to_json_value(), expected);

    Ok(())
}
