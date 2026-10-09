use loro::{ExportMode, ImportHistoryMode, LoroDoc, LoroError, VersionVector};

#[test]
fn finite_doubles_survive_json_text_then_exact_binary_import() {
    for number in [
        51.248178375505404,
        -93.31137037688033,
        2.0030397744267762e-253,
    ] {
        let a = LoroDoc::new();
        a.get_map("m").insert("n", number).unwrap();
        let json = a.export_json_updates(&VersionVector::default(), &a.oplog_vv());
        let text = serde_json::to_string(&json).unwrap();
        let b = LoroDoc::new();
        b.import_json_updates(text).unwrap();
        b.get_map("m").insert("extra", 1).unwrap();
        let updates = b.export(ExportMode::all_updates()).unwrap();
        a.import(&updates).unwrap();
        assert_eq!(a.get_deep_value(), b.get_deep_value());
    }
}

#[test]
fn exact_import_rejects_large_integers_one_apart() {
    let local = 1i64 << 60;
    let remote = local + 1;
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    a.get_map("m").insert("n", local).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(1).unwrap();
    b.get_map("m").insert("n", remote).unwrap();
    b.get_map("m").insert("extra", 1).unwrap();
    let bytes = b.export(ExportMode::all_updates()).unwrap();

    let before = a.get_deep_value();
    let vv = a.oplog_vv();
    let err = a.import(&bytes).unwrap_err();
    assert!(matches!(err, LoroError::UsedOpID { .. }));
    assert_eq!(a.get_deep_value(), before);
    assert_eq!(a.oplog_vv(), vv);
    assert!(a.oplog_vv() != b.oplog_vv() || a.get_deep_value() == b.get_deep_value());

    a.import_with_history_mode(&bytes, "", ImportHistoryMode::JsonLossy)
        .unwrap();
    assert_eq!(a.oplog_vv(), b.oplog_vv());
    assert_ne!(a.get_deep_value(), b.get_deep_value());
}
