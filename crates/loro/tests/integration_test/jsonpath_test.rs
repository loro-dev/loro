use loro::{LoroDoc, LoroList, LoroMap, LoroValue, ToJson, ValueOrContainer};
use serde_json::json;

fn to_json(v: Vec<ValueOrContainer>) -> serde_json::Value {
    v.into_iter()
        .map(|x| x.get_deep_value().to_json_value())
        .collect()
}

#[test]
fn test_filter_node_sets_preserve_empty_single_multiple_and_duplicate_results() {
    let doc = LoroDoc::new();
    doc.get_map("data")
        .insert(
            "items",
            json!([
                {"a": 1, "b": 2, "nested": {"value": 5}},
                {"b": 4, "nested": {"value": 6}},
                {"a": 3, "b": 2, "nested": {"value": 7}}
            ]),
        )
        .unwrap();
    let cases = [
        ("$.data.items[?@.missing == 1]", json!([])),
        ("$.data.items[?@.a == 1].nested.value", json!([5])),
        ("$.data.items[?@['a','b']].nested.value", json!([5, 6, 7])),
        ("$.data.items[?@.nested.value >= 6].b", json!([4, 2])),
        ("$.data.items[0,0,2].nested.value", json!([5, 5, 7])),
        ("$.data.items[*].nested.value", json!([5, 6, 7])),
    ];
    for (query, expected) in cases {
        assert_eq!(to_json(doc.jsonpath(query).unwrap()), expected, "{query}");
    }
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
    assert!(!ans.is_empty());
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
    let ans = doc.jsonpath("$..*").unwrap();
    assert_eq!(
        to_json(ans),
        serde_json::json!([
            {
                "key": 1,
                "key2": 2,
                "key3": 3
            },
            1,
            2,
            3,
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
    let ans = doc.jsonpath("$..*")?;
    assert_eq!(to_json(ans), serde_json::json!([[1, 2, 3], 1, 2, 3,]));
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


#[test]
fn parsed_queries_match_string_queries_and_preserve_duplicates() {
    use loro::jsonpath::ParsedJsonPath;

    let doc = setup_test_doc();
    for path in [
        "$", "$.missing", "$.store.book[0].author", "$.store.book[*].author",
        "$.store.book[0,0,2].author", "$.store.book[-2:].title",
        "$.store.book[0:4:2].title", "$..author",
        "$.store.book[?@.price < 10].title",
        "$.store.book[?@.price < $.store.expensive].title",
        "$.store.book[?@.missing].title",
        "$.store.book[0]['title','author','title']",
    ] {
        let query = ParsedJsonPath::parse(path).unwrap();
        assert_eq!(to_json(doc.jsonpath_parsed(&query)), to_json(doc.jsonpath(path).unwrap()), "{path}");
    }
    let query = ParsedJsonPath::parse("$.store.book[0,0,2].author").unwrap();
    assert_eq!(to_json(doc.jsonpath_parsed(&query)), json!(["Nigel Rees", "Nigel Rees", "Herman Melville"]));
}

#[test]
fn parsed_query_observes_mutations_and_can_be_reused_across_documents() {
    use loro::jsonpath::ParsedJsonPath;

    let query = ParsedJsonPath::parse("$.data.items[?@.rank >= $.data.minimum].name").unwrap();
    let first = LoroDoc::new();
    let data = first.get_map("data");
    data.insert("items", json!([{"rank":1,"name":"a"},{"rank":2,"name":"b"}])).unwrap();
    data.insert("minimum", 2).unwrap();
    assert_eq!(to_json(first.jsonpath_parsed(&query)), json!(["b"]));
    data.insert("minimum", 1).unwrap();
    assert_eq!(to_json(first.jsonpath_parsed(&query)), json!(["a", "b"]));
    data.insert("items", json!([{"rank":3,"name":"updated"}])).unwrap();
    assert_eq!(to_json(first.jsonpath_parsed(&query)), json!(["updated"]));
    let second = LoroDoc::new();
    second.get_map("data").insert("minimum", 4).unwrap();
    second.get_map("data").insert("items", json!([{"rank":5,"name":"other"}])).unwrap();
    assert_eq!(to_json(second.jsonpath_parsed(&query)), json!(["other"]));
    assert_eq!(to_json(first.jsonpath_parsed(&query)), json!(["updated"]));
}

#[test]
fn parsed_query_constructor_preserves_string_api_parse_errors() {
    use loro::jsonpath::ParsedJsonPath;

    let doc = LoroDoc::new();
    for path in ["$[", "$.data[?unknown(@.rank)]", "$[9007199254740992]"] {
        let parsed_error = ParsedJsonPath::parse(path).err().expect("invalid standard query");
        let string_error = doc.jsonpath(path).err().expect("invalid standard query");
        assert_eq!(parsed_error.to_string(), string_error.to_string(), "{path}");
    }
}


#[test]
fn jsonpath_root_names_are_literal_even_when_they_look_like_indexes_or_ids() {
    for name in ["42", "01", "1@2", "", "plain", "a/b"] {
        let doc = LoroDoc::new();
        doc.get_map(name).insert("value", name).unwrap();
        let path = format!("$[{}].value", serde_json::to_string(name).unwrap());
        assert_eq!(to_json(doc.jsonpath(&path).unwrap()), json!([name]), "{name:?}");
    }
}

#[test]
fn quoted_slash_root_does_not_alias_a_nested_path() {
    let doc = LoroDoc::new();
    doc.get_map("a/b").insert("value", "literal root").unwrap();
    doc.get_map("a").insert("b", json!({"value":"nested path"})).unwrap();
    assert_eq!(to_json(doc.jsonpath("$['a/b'].value").unwrap()), json!(["literal root"]));
    assert_eq!(to_json(doc.jsonpath("$.a.b.value").unwrap()), json!(["nested path"]));
    // The independent slash-path API retains its original meaning.
    assert_eq!(doc.get_by_str_path("a/b").unwrap().get_deep_value().to_json_value(), json!({"value":"nested path"}));
    let absent = LoroDoc::new();
    absent.get_map("a").insert("b", json!({"value":"nested only"})).unwrap();
    assert!(absent.jsonpath("$['a/b']").unwrap().is_empty());
}
