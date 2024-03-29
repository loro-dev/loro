#[test]
fn readme_basic() {
    use loro::{LoroDoc, LoroList, LoroText, LoroValue, ToJson};
    use serde_json::json;

    let doc = LoroDoc::new();
    let map = doc.get_map("map");
    map.insert("key", "value").unwrap();
    map.insert("true", true).unwrap();
    map.insert("null", LoroValue::Null).unwrap();
    map.insert("deleted", LoroValue::Null).unwrap();
    map.delete("deleted").unwrap();
    let list = map.insert_container("list", LoroList::new()).unwrap();
    list.insert(0, "List").unwrap();
    list.insert(1, 9).unwrap();
    let text = map.insert_container("text", LoroText::new()).unwrap();
    text.insert(0, "Hello world!").unwrap();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "map": {
                "key": "value",
                "true": true,
                "null": null,
                "list": ["List", 9],
                "text": "Hello world!"
            }
        })
    );
}
