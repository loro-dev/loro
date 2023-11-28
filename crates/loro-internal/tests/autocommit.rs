use loro_common::ID;
use loro_internal::{version::Frontiers, LoroDoc, ToJson};
use serde_json::json;

#[test]
fn auto_commit() {
    let mut doc_a = LoroDoc::default();
    doc_a.start_auto_commit();
    let text_a = doc_a.get_text("text");
    text_a.insert_(0, "hello").unwrap();
    text_a.delete_(2, 2).unwrap();
    assert_eq!(&**text_a.get_value().as_string().unwrap(), "heo");
    let bytes = doc_a.export_from(&Default::default());

    let mut doc_b = LoroDoc::default();
    doc_b.start_auto_commit();
    let text_b = doc_b.get_text("text");
    text_b.insert_(0, "100").unwrap();
    doc_b.import(&bytes).unwrap();
    doc_a.import(&doc_b.export_snapshot()).unwrap();
    assert_eq!(text_a.get_value(), text_b.get_value());
}

#[test]
fn auto_commit_list() {
    let mut doc_a = LoroDoc::default();
    doc_a.start_auto_commit();
    let list_a = doc_a.get_list("list");
    list_a.insert_(0, "hello").unwrap();
    assert_eq!(list_a.get_value().to_json_value(), json!(["hello"]));
    let text_a = list_a
        .insert_container_(0, loro_common::ContainerType::Text)
        .unwrap();
    let text = text_a.into_text().unwrap();
    text.insert_(0, "world").unwrap();
    let value = doc_a.get_deep_value();
    assert_eq!(value.to_json_value(), json!({"list": ["world", "hello"]}))
}

#[test]
fn auto_commit_with_checkout() {
    let mut doc = LoroDoc::default();
    doc.set_peer_id(1).unwrap();
    doc.start_auto_commit();
    let map = doc.get_map("a");
    map.insert_("0", 0).unwrap();
    map.insert_("1", 1).unwrap();
    map.insert_("2", 2).unwrap();
    map.insert_("3", 3).unwrap();
    doc.checkout(&Frontiers::from(ID::new(1, 0))).unwrap();
    assert_eq!(map.get_value().to_json_value(), json!({"0": 0}));
    // assert error if insert after checkout
    map.insert_("4", 4).unwrap_err();
    doc.checkout_to_latest();
    // assert ok if doc is attached
    map.insert_("4", 4).unwrap();
    let expected = json!({"0": 0, "1": 1, "2": 2, "3": 3, "4": 4});

    // should include all changes
    let new = LoroDoc::default();
    let a = new.get_map("a");
    new.import(&doc.export_snapshot()).unwrap();
    assert_eq!(a.get_value().to_json_value(), expected,);
}
