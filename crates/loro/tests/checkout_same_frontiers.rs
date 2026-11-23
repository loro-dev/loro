use loro::LoroDoc;

/// loro-dev/loro#1048: checking out the version a detached doc is already at
/// keeps it detached, even when that version is the latest.
#[test]
fn checkout_current_latest_version_stays_detached() {
    let doc = LoroDoc::new();
    let text = doc.get_text("t");
    let mut versions = Vec::new();
    for ch in ["a", "b", "c"] {
        text.insert(text.len_unicode(), ch).unwrap();
        doc.commit();
        versions.push(doc.state_frontiers());
    }

    for f in versions.iter().chain(versions.iter().rev()) {
        doc.checkout(f).unwrap();
        assert!(doc.is_detached(), "attached after checkout to {f:?}");
    }

    doc.try_checkout_to_latest().unwrap();
    assert!(!doc.is_detached());
    // An attached doc checking out the version it's at stays attached.
    doc.checkout(&doc.oplog_frontiers()).unwrap();
    assert!(!doc.is_detached());
}
