use loro::LoroDoc;

#[test]
fn state_only_with_frontiers_sync_after_shallow() {
    // Actor 3 performs an operation
    let doc3 = LoroDoc::new();
    doc3.set_peer_id(3).unwrap();
    doc3.get_text("text").insert(0, "hello").unwrap();
    doc3.commit();

    // Actor 0 imports shallow snapshot from Actor 3
    let doc0 = LoroDoc::new();
    doc0.set_peer_id(0).unwrap();
    let f = doc3.oplog_frontiers();
    let bytes = doc3.export(loro::ExportMode::shallow_snapshot(&f)).unwrap();
    doc0.import(&bytes).unwrap();

    // Actor 0 performs another operation
    doc0.get_text("text").insert(0, "x").unwrap();
    doc0.commit();

    eprintln!("doc0.vv = {:?}", doc0.oplog_vv());
    eprintln!("doc0.state_frontiers = {:?}", doc0.state_frontiers());

    // Actor 4 is empty, sync via state_only(Some(&state_frontiers))
    let doc4 = LoroDoc::new();
    doc4.set_peer_id(4).unwrap();
    let f = doc0.state_frontiers();
    let state_only = doc0.export(loro::ExportMode::state_only(Some(&f))).unwrap();
    eprintln!("state_only len = {}", state_only.len());
    let result = doc4.import(&state_only);
    eprintln!("import result = {:?}", result);
    eprintln!("doc4.vv = {:?}", doc4.oplog_vv());
    eprintln!("doc4.value = {:?}", doc4.get_deep_value());

    assert_eq!(doc0.get_deep_value(), doc4.get_deep_value());
}
