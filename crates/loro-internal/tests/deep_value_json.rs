use loro_common::LoroResult;
use loro_internal::{
    handler::{ListHandler, TextHandler},
    HandlerTrait, LoroDoc, TreeParentId,
};

/// Build a doc whose root map contains a list containing a text, plus a tree
/// with meta and a root counter.
fn build_doc() -> LoroResult<(LoroDoc, Vec<String>)> {
    let doc = LoroDoc::new_auto_commit();
    let map = doc.get_map("map");
    map.insert("flag", true)?;
    let list = map.insert_container("list", ListHandler::new_detached())?;
    list.insert(0, "item")?;
    let text = list.insert_container(1, TextHandler::new_detached())?;
    text.insert_unicode(0, "Hello")?;
    let tree = doc.get_tree("tree");
    let root = tree.create(TreeParentId::Root)?;
    tree.get_meta(root)?.insert("name", "root")?;
    #[cfg(feature = "counter")]
    let counter = doc.get_counter("counter");
    #[cfg(feature = "counter")]
    counter.increment(2.5)?;
    doc.commit_then_renew();

    // serde_json serializes maps in sorted key order (no `preserve_order`
    // feature in this workspace), so the pre-order cids are deterministic:
    // root keys sorted: counter < map < tree. Inside map: flag (plain) then
    // list; inside list: "item" (plain) then text.
    #[cfg(feature = "counter")]
    let cids = vec![
        counter.id().to_string(),
        map.id().to_string(),
        list.id().to_string(),
        text.id().to_string(),
        tree.id().to_string(),
    ];
    #[cfg(not(feature = "counter"))]
    let cids = vec![
        map.id().to_string(),
        list.id().to_string(),
        text.id().to_string(),
        tree.id().to_string(),
    ];
    Ok((doc, cids))
}

#[test]
fn deep_value_json_matches_plain_deep_value_serialization() -> LoroResult<()> {
    let (doc, _) = build_doc()?;
    let expected = serde_json::to_string(&doc.get_deep_value()).unwrap();
    assert_eq!(doc.get_deep_value_json()?, expected);
    Ok(())
}

#[test]
fn deep_value_json_with_ids_doc_level() -> LoroResult<()> {
    let (doc, expected_cids) = build_doc()?;
    let (json, cids) = doc.get_deep_value_json_with_ids()?;

    // json parses to the same content as the plain deep value JSON (object
    // key order may differ between the two strings; see the API docs)
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        serde_json::from_str::<serde_json::Value>(&doc.get_deep_value_json()?).unwrap(),
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        serde_json::to_value(doc.get_deep_value()).unwrap(),
    );

    // cids are in pre-order DFS of the serialized tree
    assert_eq!(cids, expected_cids);

    // tree meta maps are plain deep values: the meta container id does not
    // appear in cids
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let nodes = parsed["tree"].as_array().unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["meta"], serde_json::json!({ "name": "root" }));
    #[cfg(feature = "counter")]
    assert_eq!(parsed["counter"], serde_json::json!(2.5));
    Ok(())
}

#[test]
fn deep_value_json_with_ids_per_container() -> LoroResult<()> {
    let (doc, _) = build_doc()?;

    let map = doc.get_map("map");
    let (json, cids) = map.get_deep_value_json_with_ids()?;
    // cids[0] is the container's own id (pre-order includes the root container)
    assert_eq!(cids[0], map.id().to_string());
    assert_eq!(cids.len(), 3, "map, list, text");
    // json equals the container's plain deep value
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        serde_json::to_value(map.get_deep_value()).unwrap(),
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        serde_json::from_str::<serde_json::Value>(&map.get_deep_value_json()?).unwrap(),
    );

    let text = doc.get_text("text");
    text.insert_unicode(0, "abc")?;
    let (json, cids) = text.get_deep_value_json_with_ids()?;
    assert_eq!(json, "\"abc\"");
    assert_eq!(cids, vec![text.id().to_string()]);

    let tree = doc.get_tree("tree");
    let (json, cids) = tree.get_deep_value_json_with_ids()?;
    assert_eq!(cids, vec![tree.id().to_string()]);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        serde_json::to_value(tree.get_deep_value()).unwrap(),
    );

    #[cfg(feature = "counter")]
    {
        let counter = doc.get_counter("counter");
        let (json, cids) = counter.get_deep_value_json_with_ids()?;
        assert_eq!(json, "2.5");
        assert_eq!(cids, vec![counter.id().to_string()]);
    }
    Ok(())
}

#[test]
fn deep_value_json_empty_doc() -> LoroResult<()> {
    let doc = LoroDoc::new_auto_commit();
    assert_eq!(doc.get_deep_value_json()?, "{}");
    let (json, cids) = doc.get_deep_value_json_with_ids()?;
    assert_eq!(json, "{}");
    assert!(cids.is_empty());
    Ok(())
}

#[test]
fn deep_value_json_detached_container_errors() {
    let text = TextHandler::new_detached();
    assert!(text.get_deep_value_json().is_err());
    assert!(text.get_deep_value_json_with_ids().is_err());
    let list = ListHandler::new_detached();
    assert!(list.get_deep_value_json().is_err());
    assert!(list.get_deep_value_json_with_ids().is_err());
}
