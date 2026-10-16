use loro::LoroDoc;

#[test]
fn decoded_value_cache_configuration_is_local_and_forks_inherit_it() {
    let doc = LoroDoc::new();
    assert_eq!(doc.decoded_value_cache_limit(), 2048);
    doc.get_map("map").insert("key", "value").unwrap();
    doc.commit();
    doc.set_decoded_value_cache_limit(8192);

    let fork = doc.fork();
    let fork_at = doc.fork_at(&doc.state_frontiers()).unwrap();
    let shared = doc.clone();
    assert_eq!(fork.decoded_value_cache_limit(), 8192);
    assert_eq!(fork_at.decoded_value_cache_limit(), 8192);
    shared.set_decoded_value_cache_limit(0);
    assert_eq!(doc.decoded_value_cache_limit(), 0);
    assert_eq!(fork.decoded_value_cache_limit(), 8192);
    fork_at.set_decoded_value_cache_limit(1);
    assert_eq!(fork.decoded_value_cache_limit(), 8192);

    let restored =
        LoroDoc::from_snapshot(&doc.export(loro::ExportMode::Snapshot).unwrap()).unwrap();
    assert_eq!(restored.decoded_value_cache_limit(), 2048);
    assert_eq!(restored.get_deep_value(), doc.get_deep_value());
    doc.checkout(&doc.state_frontiers()).unwrap();
    assert_eq!(doc.fork().decoded_value_cache_limit(), 0);
}
