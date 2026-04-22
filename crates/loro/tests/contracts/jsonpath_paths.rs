#![cfg(feature = "jsonpath")]

use loro::{
    loro_value, ContainerTrait, ExportMode, Index, LoroDoc, LoroList, LoroMap, LoroValue, ToJson,
    ValueOrContainer,
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn results_json(values: Vec<ValueOrContainer>) -> Value {
    Value::Array(
        values
            .into_iter()
            .map(|value| value.get_deep_value().to_json_value())
            .collect(),
    )
}

fn strings(values: Vec<ValueOrContainer>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| {
            value
                .as_value()
                .expect("result should be a value")
                .as_string()
                .expect("result should be a string")
                .to_string()
        })
        .collect()
}

fn sorted_strings(values: Vec<ValueOrContainer>) -> Vec<String> {
    let mut values = strings(values);
    values.sort();
    values
}

fn build_catalog_doc() -> anyhow::Result<LoroDoc> {
    let doc = LoroDoc::new();
    doc.set_peer_id(97)?;

    let catalog = doc.get_map("catalog");
    let books = catalog.insert_container("books", LoroList::new())?;

    let book = books.insert_container(0, LoroMap::new())?;
    book.insert("title", "1984")?;
    book.insert("author", "George Orwell")?;
    book.insert("price", 10)?;
    book.insert("available", true)?;

    let book = books.insert_container(1, LoroMap::new())?;
    book.insert("title", "Animal Farm")?;
    book.insert("author", "George Orwell")?;
    book.insert("price", 6)?;
    book.insert("available", true)?;

    let book = books.insert_container(2, LoroMap::new())?;
    book.insert("title", "Brave New World")?;
    book.insert("author", "Aldous Huxley")?;
    book.insert("price", 12)?;
    book.insert("available", false)?;

    let book = books.insert_container(3, LoroMap::new())?;
    book.insert("title", "Fahrenheit 451")?;
    book.insert("author", "Ray Bradbury")?;
    book.insert("price", LoroValue::Null)?;
    book.insert("available", true)?;

    let book = books.insert_container(4, LoroMap::new())?;
    book.insert("title", "Pride and Prejudice")?;
    book.insert("author", "Jane Austen")?;
    book.insert("price", 7)?;
    book.insert("available", true)?;

    let featured_authors = catalog.insert_container("featured_authors", LoroList::new())?;
    featured_authors.push("George Orwell")?;
    featured_authors.push("Jane Austen")?;
    catalog.insert("featured_author", "George Orwell")?;
    catalog.insert("min_price", 9)?;

    let special_keys = catalog.insert_container("special keys", LoroMap::new())?;
    special_keys.insert("spaced key", "space")?;
    special_keys.insert("quote'key", "single quote")?;
    special_keys.insert("line\nbreak", "line break")?;
    special_keys.insert("emoji \u{1F600}", "smile")?;

    let departments = catalog.insert_container("departments", LoroList::new())?;
    let ops = departments.insert_container(0, LoroMap::new())?;
    ops.insert("name", "ops")?;
    let ops_items = ops.insert_container("items", LoroList::new())?;
    let item = ops_items.insert_container(0, LoroMap::new())?;
    item.insert("name", "audit")?;
    item.insert("done", false)?;
    item.insert("priority", 1)?;

    let eng = departments.insert_container(1, LoroMap::new())?;
    eng.insert("name", "eng")?;
    let eng_items = eng.insert_container("items", LoroList::new())?;
    let item = eng_items.insert_container(0, LoroMap::new())?;
    item.insert("name", "release")?;
    item.insert("done", true)?;
    item.insert("priority", 3)?;

    doc.commit();
    Ok(doc)
}

#[test]
fn jsonpath_recursive_descent_wildcard_union_and_slices_follow_contract() -> anyhow::Result<()> {
    let doc = build_catalog_doc()?;

    assert_eq!(
        sorted_strings(doc.jsonpath("$.catalog..author")?),
        vec![
            "Aldous Huxley".to_string(),
            "George Orwell".to_string(),
            "George Orwell".to_string(),
            "Jane Austen".to_string(),
            "Ray Bradbury".to_string(),
        ]
    );
    assert_eq!(doc.jsonpath("$.catalog.*")?.len(), 6);
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[*].title")?),
        vec![
            "1984".to_string(),
            "Animal Farm".to_string(),
            "Brave New World".to_string(),
            "Fahrenheit 451".to_string(),
            "Pride and Prejudice".to_string(),
        ]
    );
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[4,2,0].title")?),
        vec![
            "Pride and Prejudice".to_string(),
            "Brave New World".to_string(),
            "1984".to_string(),
        ]
    );
    assert_eq!(
        results_json(doc.jsonpath("$.catalog['featured_author','min_price']")?),
        json!(["George Orwell", 9])
    );
    assert_eq!(
        results_json(doc.jsonpath("$['catalog']['special keys']['quote\\'key']")?),
        json!(["single quote"])
    );
    assert_eq!(
        results_json(doc.jsonpath("$.catalog[\"special keys\"][\"line\\nbreak\"]")?),
        json!(["line break"])
    );
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[-2:].title")?),
        vec![
            "Fahrenheit 451".to_string(),
            "Pride and Prejudice".to_string(),
        ]
    );
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[0:5:2].title")?),
        vec![
            "1984".to_string(),
            "Brave New World".to_string(),
            "Pride and Prejudice".to_string(),
        ]
    );
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[4:0:-2].title")?),
        vec![
            "Pride and Prejudice".to_string(),
            "Brave New World".to_string(),
        ]
    );
    assert!(doc.jsonpath("$.catalog.books[2:2].title")?.is_empty());

    Ok(())
}

#[test]
fn jsonpath_filters_cover_root_and_current_refs_for_bool_null_number_and_string_values(
) -> anyhow::Result<()> {
    let doc = build_catalog_doc()?;

    assert_eq!(
        strings(
            doc.jsonpath(
                "$.catalog.books[?(@.author == $.catalog.featured_author && @.available == true)].title"
            )?
        ),
        vec!["1984".to_string(), "Animal Farm".to_string()]
    );
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[?(@.author in $.catalog.featured_authors)].title")?),
        vec![
            "1984".to_string(),
            "Animal Farm".to_string(),
            "Pride and Prejudice".to_string(),
        ]
    );
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[?(@.price == null)].title")?),
        vec!["Fahrenheit 451".to_string()]
    );
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[?(@.price >= $.catalog.min_price)].title")?),
        vec!["1984".to_string(), "Brave New World".to_string()]
    );
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[?(@.title contains 'Farm')].title")?),
        vec!["Animal Farm".to_string()]
    );

    Ok(())
}

#[test]
fn jsonpath_filter_functions_literals_and_empty_path_results_follow_contract() -> anyhow::Result<()>
{
    let doc = build_catalog_doc()?;

    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[?(count(@.isbn) == 0)].title")?),
        vec![
            "1984".to_string(),
            "Animal Farm".to_string(),
            "Brave New World".to_string(),
            "Fahrenheit 451".to_string(),
            "Pride and Prejudice".to_string(),
        ]
    );
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[?(length(value(@.title)) > 10)].title")?),
        vec![
            "Animal Farm".to_string(),
            "Brave New World".to_string(),
            "Fahrenheit 451".to_string(),
            "Pride and Prejudice".to_string(),
        ]
    );
    assert_eq!(
        strings(doc.jsonpath(
            "$.catalog.books[?(value(@.author) in ['George Orwell', 'Ray Bradbury'])].title"
        )?),
        vec![
            "1984".to_string(),
            "Animal Farm".to_string(),
            "Fahrenheit 451".to_string(),
        ]
    );
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[?(@.price in [null, 6, 12])].title")?),
        vec![
            "Animal Farm".to_string(),
            "Brave New World".to_string(),
            "Fahrenheit 451".to_string(),
        ]
    );
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[?(@.available != false && @.title >= 'F')].title")?),
        vec![
            "Fahrenheit 451".to_string(),
            "Pride and Prejudice".to_string(),
        ]
    );
    assert!(doc
        .jsonpath("$.catalog.books[?(@.missing == $.catalog.missing)].title")?
        .is_empty());
    assert!(doc
        .jsonpath("$.catalog.books[?(@.price in $.catalog.missing)].title")?
        .is_empty());

    Ok(())
}

#[test]
fn jsonpath_comparison_matrix_and_value_function_follow_contract() -> anyhow::Result<()> {
    let doc = build_catalog_doc()?;
    let records = doc.get_list("records");
    let flags = records.insert_container(0, LoroMap::new())?;
    flags.insert("truthy_string", "non-empty")?;
    flags.insert("empty_string", "")?;
    flags.insert("truthy_int", 7)?;
    flags.insert("zero_int", 0)?;
    flags.insert("truthy_float", 1.5)?;
    flags.insert("zero_float", 0.0)?;
    flags.insert("truthy_bool", true)?;
    flags.insert("false_bool", false)?;
    flags.insert("null_value", LoroValue::Null)?;
    let list = flags.insert_container("list", LoroList::new())?;
    list.push("item")?;
    flags.insert_container("empty_list", LoroList::new())?;
    let map = flags.insert_container("map", LoroMap::new())?;
    map.insert("key", "value")?;
    flags.insert_container("empty_map", LoroMap::new())?;
    doc.commit();

    for query in [
        "$.records[?(value(@.truthy_string) == 'non-empty')].truthy_string",
        "$.records[?(value(@.empty_string) == '')].truthy_string",
        "$.records[?(value(@.truthy_int) == 7)].truthy_string",
        "$.records[?(value(@.zero_int) == 0)].truthy_string",
        "$.records[?(value(@.truthy_float) == 1.5)].truthy_string",
        "$.records[?(value(@.zero_float) == 0.0)].truthy_string",
        "$.records[?(value(@.truthy_bool) == true)].truthy_string",
        "$.records[?(value(@.false_bool) == false)].truthy_string",
        "$.records[?(value(@.null_value) == null)].truthy_string",
        "$.records[?(length(value(@.list)) == 1)].truthy_string",
        "$.records[?(length(value(@.empty_list)) == 0)].truthy_string",
        "$.records[?(length(value(@.map)) == 1)].truthy_string",
        "$.records[?(length(value(@.empty_map)) == 0)].truthy_string",
    ] {
        assert_eq!(
            results_json(doc.jsonpath(query)?),
            json!(["non-empty"]),
            "{query}"
        );
    }

    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[?(count(@.title) == 1)].title")?),
        vec![
            "1984".to_string(),
            "Animal Farm".to_string(),
            "Brave New World".to_string(),
            "Fahrenheit 451".to_string(),
            "Pride and Prejudice".to_string(),
        ]
    );
    assert_eq!(
        results_json(doc.jsonpath("$.catalog.books[?(value(@.title) == '1984')]")?),
        json!([{
            "title": "1984",
            "author": "George Orwell",
            "price": 10,
            "available": true,
        }])
    );
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[?(value(@.*) == null)].title")?),
        vec![
            "1984".to_string(),
            "Animal Farm".to_string(),
            "Brave New World".to_string(),
            "Fahrenheit 451".to_string(),
            "Pride and Prejudice".to_string(),
        ]
    );

    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[?(@.price != 10 && @.price <= 7)].title")?),
        vec!["Animal Farm".to_string(), "Pride and Prejudice".to_string(),]
    );
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[?(@.price < 10.5 && @.price >= 6)].title")?),
        vec![
            "1984".to_string(),
            "Animal Farm".to_string(),
            "Pride and Prejudice".to_string(),
        ]
    );
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[?(@.title <= 'Animal Farm')].title")?),
        vec!["1984".to_string(), "Animal Farm".to_string()]
    );
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[?(@.title > 'F')].title")?),
        vec![
            "Fahrenheit 451".to_string(),
            "Pride and Prejudice".to_string()
        ]
    );
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[?(@.available != true)].title")?),
        vec!["Brave New World".to_string()]
    );
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[?(@.author == @.author)].title")?),
        vec![
            "1984".to_string(),
            "Animal Farm".to_string(),
            "Brave New World".to_string(),
            "Fahrenheit 451".to_string(),
            "Pride and Prejudice".to_string(),
        ]
    );
    assert!(doc
        .jsonpath("$.catalog.books[?(@.missing == 1)].title")?
        .is_empty());
    assert_eq!(
        strings(doc.jsonpath("$.catalog.books[?(length(value(@)) == 4)].title")?),
        vec![
            "1984".to_string(),
            "Animal Farm".to_string(),
            "Brave New World".to_string(),
            "Fahrenheit 451".to_string(),
            "Pride and Prejudice".to_string(),
        ]
    );

    Ok(())
}

#[test]
fn jsonpath_root_wildcards_negative_indexes_and_scalar_paths_follow_contract() -> anyhow::Result<()>
{
    let doc = build_catalog_doc()?;

    assert_eq!(doc.jsonpath("$.*")?.len(), 1);
    assert_eq!(
        results_json(doc.jsonpath("$.catalog.books[-1].title")?),
        json!(["Pride and Prejudice"])
    );
    assert!(doc.jsonpath("$.catalog.books[-99].title")?.is_empty());
    assert!(doc.jsonpath("$.catalog.featured_author[0]")?.is_empty());
    assert!(doc.jsonpath("$.catalog.featured_authors.title")?.is_empty());
    assert_eq!(
        results_json(doc.jsonpath("$.catalog.books[::-2].title")?),
        json!(["Pride and Prejudice", "Brave New World", "1984"])
    );
    assert_eq!(
        results_json(doc.jsonpath("$.catalog.books[99:120].title")?),
        json!([])
    );

    Ok(())
}

#[test]
fn jsonpath_queries_json_values_embedded_in_containers_follow_contract() -> anyhow::Result<()> {
    let doc = build_catalog_doc()?;
    let catalog = doc.get_map("catalog");
    catalog.insert(
        "inline",
        loro_value!({
            "records": [
                {"name": "alpha", "score": 1, "enabled": true},
                {"name": "beta", "score": 2, "enabled": false},
                {"name": "gamma", "score": 3, "enabled": true}
            ],
            "nested": {
                "flags": [true, false],
                "label": "embedded"
            },
            "empty": []
        }),
    )?;
    doc.commit();

    assert_eq!(
        results_json(doc.jsonpath("$.catalog.inline.records[*].name")?),
        json!(["alpha", "beta", "gamma"])
    );
    assert_eq!(
        results_json(doc.jsonpath("$.catalog.inline.records[-1].score")?),
        json!([3])
    );
    assert_eq!(
        results_json(doc.jsonpath("$.catalog.inline.records[?(@.score >= 2)].name")?),
        json!(["beta", "gamma"])
    );
    assert_eq!(
        results_json(doc.jsonpath("$.catalog.inline.records[?(@.enabled == true)].name")?),
        json!(["alpha", "gamma"])
    );
    assert_eq!(
        results_json(doc.jsonpath("$.catalog.inline.nested.flags[0]")?),
        json!([true])
    );
    assert_eq!(
        results_json(doc.jsonpath("$.catalog.inline.nested[?(@ == 'embedded')]")?),
        json!(["embedded"])
    );
    assert!(doc.jsonpath("$.catalog.inline.empty[*]")?.is_empty());

    let replica = LoroDoc::new();
    replica.import(&doc.export(ExportMode::all_updates())?)?;
    assert_eq!(
        results_json(replica.jsonpath("$.catalog.inline.records[1:].name")?),
        json!(["beta", "gamma"])
    );

    Ok(())
}

#[test]
fn jsonpath_nested_subscriptions_follow_deep_paths_and_roundtrip_cleanly() -> anyhow::Result<()> {
    let doc = build_catalog_doc()?;

    let hits = Arc::new(AtomicUsize::new(0));
    let hits_ref = Arc::clone(&hits);
    let sub = doc.subscribe_jsonpath(
        "$.catalog.departments[*].items[*].name",
        Arc::new(move || {
            hits_ref.fetch_add(1, Ordering::SeqCst);
        }),
    )?;

    let catalog = doc.get_map("catalog");
    let departments = catalog
        .get("departments")
        .unwrap()
        .into_container()
        .unwrap()
        .into_list()
        .unwrap();
    let ops = departments
        .get(0)
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap();
    let ops_items = ops
        .get("items")
        .unwrap()
        .into_container()
        .unwrap()
        .into_list()
        .unwrap();
    let new_item = ops_items.insert_container(1, LoroMap::new())?;
    new_item.insert("name", "ship")?;
    new_item.insert("done", false)?;
    new_item.insert("priority", 2)?;
    doc.commit();

    assert!(hits.load(Ordering::SeqCst) >= 1);

    let nested = doc.jsonpath("$.catalog.departments[0].items[1]")?;
    assert_eq!(nested.len(), 1);
    let nested_container = nested[0].as_container().expect("expected nested container");
    let path = doc
        .get_path_to_container(&nested_container.id())
        .expect("nested container should still be attached");
    assert_eq!(path.last().map(|(_, index)| index), Some(&Index::Seq(1)));
    assert_eq!(
        doc.get_by_path(&[
            Index::Key("catalog".into()),
            Index::Key("departments".into()),
            Index::Seq(0),
            Index::Key("items".into()),
            Index::Seq(1),
            Index::Key("name".into()),
        ])
        .expect("nested path should resolve")
        .get_deep_value()
        .to_json_value(),
        json!("ship")
    );

    let snapshot = LoroDoc::from_snapshot(&doc.export(ExportMode::Snapshot)?)?;
    assert_eq!(
        results_json(snapshot.jsonpath("$.catalog.departments[0].items[1].name")?),
        json!(["ship"])
    );
    let snapshot_nested = snapshot.jsonpath("$.catalog.departments[0].items[1]")?;
    let snapshot_container = snapshot_nested[0]
        .as_container()
        .expect("snapshot nested container should exist");
    let snapshot_path = snapshot
        .get_path_to_container(&snapshot_container.id())
        .expect("snapshot nested container should still be attached");
    assert_eq!(
        snapshot_path.last().map(|(_, index)| index),
        Some(&Index::Seq(1))
    );

    let replica = LoroDoc::new();
    replica.import(&doc.export(ExportMode::all_updates())?)?;
    assert_eq!(
        results_json(replica.jsonpath("$.catalog.departments[0].items[1].name")?),
        json!(["ship"])
    );
    let replica_nested = replica.jsonpath("$.catalog.departments[0].items[1]")?;
    let replica_container = replica_nested[0]
        .as_container()
        .expect("replica nested container should exist");
    let replica_path = replica
        .get_path_to_container(&replica_container.id())
        .expect("replica nested container should still be attached");
    assert_eq!(
        replica_path.last().map(|(_, index)| index),
        Some(&Index::Seq(1))
    );

    sub.unsubscribe();
    let hits_after_unsubscribe = hits.load(Ordering::SeqCst);
    new_item.insert("name", "ship v2")?;
    doc.commit();
    assert_eq!(hits.load(Ordering::SeqCst), hits_after_unsubscribe);

    Ok(())
}

#[test]
fn jsonpath_invalid_syntax_and_type_errors_are_reported() -> anyhow::Result<()> {
    let doc = build_catalog_doc()?;

    for invalid in [
        "catalog.books",
        "$.catalog.books[",
        "$.catalog.books[?(@.price <)]",
        "$.catalog.books[9223372036854775808]",
    ] {
        assert!(
            doc.jsonpath(invalid).is_err(),
            "{invalid} should be rejected"
        );
    }

    for invalid in [
        "$.catalog.books[?count(1)]",
        "$.catalog.books[?count(@.title, @.author)]",
        "$.catalog.books[?foo(@.title)]",
        "$.catalog.books[?(@.title == $.catalog.books[*].title)]",
    ] {
        assert!(
            doc.jsonpath(invalid).is_err(),
            "{invalid} should be rejected"
        );
    }

    Ok(())
}
