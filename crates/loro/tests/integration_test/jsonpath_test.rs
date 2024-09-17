use loro::{
    ExportMode, Frontiers, LoroDoc, LoroList, LoroMap, LoroValue, ToJson, ValueOrContainer, ID,
};
use serde_json::json;

fn to_json(v: Vec<ValueOrContainer>) -> serde_json::Value {
    v.into_iter()
        .map(|x| x.get_deep_value().to_json_value())
        .collect()
}

fn create_map_from_json(json: serde_json::Value) -> LoroMap {
    let map = LoroMap::new();
    for (key, value) in json.as_object().unwrap().iter() {
        map.insert(key, value.clone()).unwrap();
    }
    map
}

fn setup_test_doc() -> LoroDoc {
    let doc = LoroDoc::new();
    let store = doc.get_map("store");

    let books = store.insert_container("book", LoroList::new()).unwrap();
    books
        .insert_container(
            0,
            create_map_from_json(json!({
                "category": "reference",
                "author": "Nigel Rees",
                "title": "Sayings of the Century",
                "price": 8.95,
                "isbn": "0-553-21311-3"
            })),
        )
        .unwrap();
    books
        .insert_container(
            1,
            create_map_from_json(json!({
                "category": "fiction",
                "author": "Evelyn Waugh",
                "title": "Sword of Honour",
                "price": 12.99,
                "isbn": "0-553-21312-1"
            })),
        )
        .unwrap();
    books
        .insert_container(
            2,
            create_map_from_json(json!({
                "category": "fiction",
                "author": "Herman Melville",
                "title": "Moby Dick",
                "price": 8.99,
                "isbn": "0-553-21313-X"
            })),
        )
        .unwrap();
    books
        .insert_container(
            3,
            create_map_from_json(json!({
                "category": "fiction",
                "author": "J. R. R. Tolkien",
                "title": "The Lord of the Rings",
                "price": 22.99,
                "isbn": "0-395-19395-8"
            })),
        )
        .unwrap();

    store
        .insert_container(
            "bicycle",
            create_map_from_json(json!({
                "color": "red",
                "price": 19.95
            })),
        )
        .unwrap();

    store.insert("expensive", 10).unwrap();

    doc
}

#[test]
fn test_all_authors() -> anyhow::Result<()> {
    let doc = setup_test_doc();
    let ans = doc.jsonpath("$.store.book[*].author")?;
    assert_eq!(
        to_json(ans),
        json!([
            "Nigel Rees",
            "Evelyn Waugh",
            "Herman Melville",
            "J. R. R. Tolkien"
        ])
    );
    Ok(())
}

#[test]
#[ignore = "filter syntax not implemented"]
fn test_books_with_isbn() -> anyhow::Result<()> {
    let doc = setup_test_doc();
    let ans = doc.jsonpath("$..book[?(@.isbn)]")?;
    assert_eq!(ans.len(), 4);
    Ok(())
}

#[test]
fn test_all_things_in_store() -> anyhow::Result<()> {
    let doc = setup_test_doc();
    let ans = doc.jsonpath("$.store.*")?;
    assert_eq!(ans.len(), 3); // book array, bicycle object, and expensive value
    Ok(())
}

#[test]
fn test_all_authors_recursive() -> anyhow::Result<()> {
    let doc = setup_test_doc();
    let ans = doc.jsonpath("$..author")?;
    assert_eq!(
        to_json(ans),
        json!([
            "Nigel Rees",
            "Evelyn Waugh",
            "Herman Melville",
            "J. R. R. Tolkien"
        ])
    );
    Ok(())
}

#[test]
fn test_all_prices() -> anyhow::Result<()> {
    let doc = setup_test_doc();
    let ans = doc.jsonpath("$.store..price")?;
    assert_eq!(to_json(ans), json!([19.95, 8.95, 12.99, 8.99, 22.99]));
    Ok(())
}

#[test]
fn test_third_book() -> anyhow::Result<()> {
    let doc = setup_test_doc();
    let ans = doc.jsonpath("$..book[2]")?;
    assert_eq!(
        to_json(ans),
        json!([{
            "category": "fiction",
            "author": "Herman Melville",
            "title": "Moby Dick",
            "price": 8.99,
            "isbn": "0-553-21313-X"
        }])
    );
    Ok(())
}

#[test]
fn test_second_to_last_book() -> anyhow::Result<()> {
    let doc = setup_test_doc();
    let ans = doc.jsonpath("$..book[-2]")?;
    assert_eq!(
        to_json(ans),
        json!([{
            "category": "fiction",
            "author": "Herman Melville",
            "title": "Moby Dick",
            "price": 8.99,
            "isbn": "0-553-21313-X"
        }])
    );
    Ok(())
}

#[test]
fn test_first_two_books() -> anyhow::Result<()> {
    let doc = setup_test_doc();
    let ans = doc.jsonpath("$..book[0,1]")?;
    assert_eq!(ans.len(), 2);
    Ok(())
}

#[test]
fn test_books_slice() -> anyhow::Result<()> {
    let doc = setup_test_doc();
    let ans = doc.jsonpath("$..book[:2]")?;
    assert_eq!(ans.len(), 2);
    Ok(())
}

#[test]
fn test_books_slice_from_index() -> anyhow::Result<()> {
    let doc = setup_test_doc();
    let ans = doc.jsonpath("$..book[1:2]")?;
    assert_eq!(ans.len(), 1);
    Ok(())
}

#[test]
fn test_last_two_books() -> anyhow::Result<()> {
    let doc = setup_test_doc();
    let ans = doc.jsonpath("$..book[-2:]")?;
    assert_eq!(ans.len(), 2);
    Ok(())
}

#[test]
fn test_book_number_two_from_tail() -> anyhow::Result<()> {
    let doc = setup_test_doc();
    let ans = doc.jsonpath("$..book[2:]")?;
    assert_eq!(ans.len(), 2);
    Ok(())
}

#[test]
#[ignore = "filter syntax not implemented"]
fn test_books_cheaper_than_10() -> anyhow::Result<()> {
    let doc = setup_test_doc();
    let ans = doc.jsonpath("$.store.book[?(@.price < 10)]")?;
    assert_eq!(ans.len(), 2);
    Ok(())
}

#[test]
#[ignore = "filter syntax not implemented"]
fn test_books_not_expensive() -> anyhow::Result<()> {
    let doc = setup_test_doc();
    let ans = doc.jsonpath("$..book[?(@.price <= $.expensive)]")?;
    assert_eq!(ans.len(), 2);
    Ok(())
}

#[test]
fn test_everything() -> anyhow::Result<()> {
    let doc = setup_test_doc();
    let ans = doc.jsonpath("$..*")?;
    assert!(ans.len() > 0);
    Ok(())
}

#[test]
fn test_books_slice_with_step() -> anyhow::Result<()> {
    let doc = setup_test_doc();
    let ans = doc.jsonpath("$..book[1:9:3]")?;
    assert_eq!(ans.len(), 1);
    Ok(())
}

#[test]
fn test_multiple_keys() -> anyhow::Result<()> {
    let doc = setup_test_doc();
    let ans = doc.jsonpath("$.store[\"book\", \"bicycle\"]")?;
    assert_eq!(ans.len(), 2);
    Ok(())
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
