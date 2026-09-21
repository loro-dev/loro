//! Inserting a container attached to another doc must return `Err` without
//! touching the target doc. This misuse used to panic with
//! "Parent is not registered".

use loro::{
    ContainerTrait, LoroCounter, LoroDoc, LoroError, LoroList, LoroMap, LoroMovableList, LoroText,
    LoroTree, LoroValue, ToJson,
};

fn source_doc() -> LoroDoc {
    let doc = LoroDoc::new();
    doc.get_text("text").insert(0, "cross").unwrap();
    let map = doc.get_map("map");
    map.insert("k", 1).unwrap();
    map.insert_container("child", LoroText::new())
        .unwrap()
        .insert(0, "nested")
        .unwrap();
    doc.get_list("list").push(1).unwrap();
    doc.get_movable_list("movable").push(1).unwrap();
    let tree = doc.get_tree("tree");
    let node = tree.create(None).unwrap();
    tree.get_meta(node).unwrap().insert("name", "n").unwrap();
    doc.get_counter("counter").increment(2.).unwrap();
    doc.commit();
    doc
}

fn target_doc() -> LoroDoc {
    let doc = LoroDoc::new();
    doc.get_list("list").push(0).unwrap();
    doc.get_movable_list("movable").push(0).unwrap();
    doc.get_map("map").insert("x", 0).unwrap();
    doc.commit();
    doc
}

fn assert_rejected<T>(result: Result<T, LoroError>) {
    match result {
        Err(LoroError::ArgErr(msg)) => assert!(msg.contains("another LoroDoc"), "{msg}"),
        Err(e) => panic!("expected ArgErr, got {e:?}"),
        Ok(_) => panic!("expected ArgErr, got Ok"),
    }
}

/// Try every insertion entry point with `child`, checking that each one is
/// rejected and leaves `target` unchanged and usable.
fn check_all_entry_points<C: ContainerTrait + Clone>(child: C) {
    let target = target_doc();
    let before = target.get_deep_value().to_json_value();
    let vv = target.oplog_vv();

    assert_rejected(target.get_list("list").insert_container(0, child.clone()));
    assert_rejected(target.get_list("list").push_container(child.clone()));
    assert_rejected(
        target
            .get_movable_list("movable")
            .insert_container(0, child.clone()),
    );
    assert_rejected(
        target
            .get_movable_list("movable")
            .set_container(0, child.clone()),
    );
    assert_rejected(target.get_map("map").insert_container("c", child.clone()));

    target.commit();
    assert_eq!(target.get_deep_value().to_json_value(), before);
    assert_eq!(target.oplog_vv(), vv);

    // The target doc stays usable.
    target.get_text("still-usable").insert(0, "ok").unwrap();
    target.commit();
    assert_eq!(
        target.get_text("still-usable").to_string(),
        "ok".to_string()
    );
}

#[test]
fn reject_attached_containers_from_another_doc() {
    let src = source_doc();
    check_all_entry_points(src.get_text("text"));
    check_all_entry_points(src.get_map("map"));
    check_all_entry_points(src.get_list("list"));
    check_all_entry_points(src.get_movable_list("movable"));
    check_all_entry_points(src.get_tree("tree"));
    check_all_entry_points(src.get_counter("counter"));
    // Source doc is untouched as well.
    assert_eq!(src.get_text("text").to_string(), "cross");
}

#[test]
fn reject_detached_containers_holding_foreign_children() {
    let src = source_doc();
    let foreign = src.get_text("text");

    let map = LoroMap::new();
    map.insert("v", 1).unwrap();
    map.insert_container("t", foreign.clone()).unwrap();
    check_all_entry_points(map);

    let list = LoroList::new();
    list.push(1).unwrap();
    let inner = list.push_container(LoroMap::new()).unwrap();
    inner.insert_container("t", foreign.clone()).unwrap();
    check_all_entry_points(list);

    let movable = LoroMovableList::new();
    movable.push_container(foreign.clone()).unwrap();
    check_all_entry_points(movable);

    let tree = LoroTree::new();
    let node = tree.create(None).unwrap();
    tree.get_meta(node)
        .unwrap()
        .insert_container("t", foreign)
        .unwrap();
    check_all_entry_points(tree);
}

#[test]
fn same_doc_attached_copy_still_works() {
    let doc = source_doc();
    let list = doc.get_list("copies");
    list.insert_container(0, doc.get_map("map")).unwrap();
    list.insert_container(1, doc.get_counter("counter"))
        .unwrap();
    doc.commit();
    let value = list.get_deep_value();
    let LoroValue::List(items) = value else {
        panic!("{value:?}")
    };
    assert_eq!(items[0], doc.get_map("map").get_deep_value());
    assert_eq!(items[1], LoroValue::Double(2.));

    // Detached containers without foreign children keep working too.
    let text = LoroText::new();
    text.insert(0, "detached").unwrap();
    let counter = LoroCounter::new();
    counter.increment(1.).unwrap();
    let target = LoroDoc::new();
    target.get_map("m").insert_container("t", text).unwrap();
    target.get_map("m").insert_container("c", counter).unwrap();
    assert_eq!(
        target.get_deep_value().to_json_value(),
        serde_json::json!({"m": {"t": "detached", "c": 1.0}})
    );
}
