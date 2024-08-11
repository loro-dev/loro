use loro_common::ID;
use loro_internal::{version::Frontiers, HandlerTrait, LoroDoc, TextHandler, ToJson};
use serde_json::json;

#[test]
fn auto_commit() {
    let doc_a = LoroDoc::default();
    doc_a.set_peer_id(1).unwrap();
    doc_a.start_auto_commit();
    let text_a = doc_a.get_text("text");
    text_a.insert(0, "hello").unwrap();
    text_a.delete(2, 2).unwrap();
    assert_eq!(text_a.get_value().as_string().unwrap().0, "heo");
    let bytes = doc_a.export_from(&Default::default());

    let doc_b = LoroDoc::default();
    doc_b.start_auto_commit();
    doc_b.set_peer_id(2).unwrap();
    let text_b = doc_b.get_text("text");
    text_b.insert(0, "100").unwrap();
    doc_b.import(&bytes).unwrap();
    doc_a.import(&doc_b.export_snapshot()).unwrap();
    assert_eq!(text_a.get_value(), text_b.get_value());
    doc_a.check_state_diff_calc_consistency_slow();
}

#[test]
fn auto_commit_list() {
    let doc_a = LoroDoc::default();
    doc_a.start_auto_commit();
    let list_a = doc_a.get_list("list");
    list_a.insert(0, "hello").unwrap();
    assert_eq!(list_a.get_value().to_json_value(), json!(["hello"]));
    let text_a = list_a
        .insert_container(0, TextHandler::new_detached())
        .unwrap();
    let text = text_a;
    text.insert(0, "world").unwrap();
    let value = doc_a.get_deep_value();
    assert_eq!(value.to_json_value(), json!({"list": ["world", "hello"]}))
}

#[test]
fn auto_commit_with_checkout() {
    let doc = LoroDoc::default();
    doc.set_peer_id(1).unwrap();
    doc.start_auto_commit();
    let map = doc.get_map("a");
    map.insert("0", 0).unwrap();
    map.insert("1", 1).unwrap();
    map.insert("2", 2).unwrap();
    map.insert("3", 3).unwrap();
    doc.checkout(&Frontiers::from(ID::new(1, 0))).unwrap();
    assert_eq!(map.get_value().to_json_value(), json!({"0": 0}));
    // assert error if insert after checkout
    map.insert("4", 4).unwrap_err();
    doc.checkout_to_latest();
    // assert ok if doc is attached
    map.insert("4", 4).unwrap();
    let expected = json!({"0": 0, "1": 1, "2": 2, "3": 3, "4": 4});

    // should include all changes
    let new = LoroDoc::default();
    let a = new.get_map("a");
    new.import(&doc.export_snapshot()).unwrap();
    assert_eq!(a.get_value().to_json_value(), expected,);
}
