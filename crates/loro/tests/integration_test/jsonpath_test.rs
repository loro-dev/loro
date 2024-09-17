use loro::{ExportMode, Frontiers, LoroDoc, LoroMap, LoroValue, ToJson, ValueOrContainer, ID};

fn to_json(v: Vec<ValueOrContainer>) -> serde_json::Value {
    v.into_iter()
        .map(|x| x.get_deep_value().to_json_value())
        .collect()
}

#[test]
fn test_jsonpath() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.get_map("root").insert("key", LoroValue::from(1))?;
    doc.get_map("root").insert("key2", LoroValue::from(2))?;
    doc.get_map("root").insert("key3", LoroValue::from(3))?;
    let ans = doc.jsonpath("$..").unwrap();
    assert_eq!(
        to_json(ans),
        serde_json::json!([
            1,
            2,
            3,
            {
                "key": 1,
                "key2": 2,
                "key3": 3
            },
            {
                "root": {
                    "key": 1,
                    "key2": 2,
                    "key3": 3
                }
            }
        ])
    );
    Ok(())
}

#[test]
fn test_jsonpath_with_array() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let array = doc.get_list("root");
    array.insert(0, 1)?;
    array.insert(1, 2)?;
    array.insert(2, 3)?;
    let ans = doc.jsonpath("$..")?;
    assert_eq!(
        to_json(ans),
        serde_json::json!([
            1,
            2,
            3,
            [1, 2, 3],
            { "root": [1, 2, 3] }
        ])
    );
    Ok(())
}

#[test]
fn test_jsonpath_nested_objects() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let root = doc.get_map("root");
    let child = root.insert_container("child", LoroMap::new())?;
    child.insert("key", "value")?;
    let ans = doc.jsonpath("$.root.child.key").unwrap();
    assert_eq!(to_json(ans), serde_json::json!(["value"]));
    Ok(())
}

#[test]
fn test_jsonpath_wildcard() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let root = doc.get_map("root");
    root.insert("key1", 1)?;
    root.insert("key2", 2)?;
    root.insert("key3", 3)?;
    let ans = doc.jsonpath("$.root.*").unwrap();
    assert_eq!(to_json(ans), serde_json::json!([1, 2, 3]));
    Ok(())
}
