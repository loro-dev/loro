use loro::{ExportMode, LoroDoc, VersionVector};

#[test]
fn state_vv_keeps_its_infallible_signature_and_tracks_checked_out_state() {
    // Downstream code written before 1.17.0 must continue compiling.
    let state_vv: fn(&LoroDoc) -> VersionVector = LoroDoc::state_vv;
    let source = LoroDoc::new();
    source.set_peer_id(1).unwrap();
    source.get_text("text").insert(0, "a").unwrap();
    source.commit();
    let earlier_frontiers = source.state_frontiers();
    let earlier_vv = state_vv(&source);
    source.get_text("text").insert(1, "b").unwrap();
    let snapshot = source.export(ExportMode::Snapshot).unwrap();

    let doc = LoroDoc::new();
    doc.import(&snapshot).unwrap();
    let latest_vv = doc.oplog_vv();
    assert_ne!(earlier_vv, latest_vv);
    assert_eq!(state_vv(&doc), latest_vv);
    assert_eq!(doc.try_state_vv().unwrap(), latest_vv);

    doc.checkout(&earlier_frontiers).unwrap();
    assert_eq!(doc.get_text("text").to_string(), "a");
    assert_eq!(state_vv(&doc), earlier_vv);
    assert_eq!(doc.try_state_vv().unwrap(), earlier_vv);
    assert_eq!(doc.oplog_vv(), latest_vv);

    doc.checkout_to_latest();
    assert_eq!(state_vv(&doc), latest_vv);
    assert_eq!(doc.try_state_vv().unwrap(), latest_vv);
}
