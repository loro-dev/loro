#![cfg(feature = "jsonpath")]

use loro::{ExportMode, LoroDoc, LoroList, LoroMap, LoroValue, ToJson, ValueOrContainer};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn titles(values: Vec<ValueOrContainer>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.as_value().unwrap().as_string().unwrap().to_string())
        .collect()
}

fn deep_json(values: Vec<ValueOrContainer>) -> Value {
    Value::Array(
        values
            .into_iter()
            .map(|value| value.get_deep_value().to_json_value())
            .collect(),
    )
}

fn build_doc() -> anyhow::Result<LoroDoc> {
    let doc = LoroDoc::new();
    doc.set_peer_id(17)?;

    let store = doc.get_map("store");
    let books = store.insert_container("books", LoroList::new())?;

    let book = books.insert_container(0, LoroMap::new())?;
    book.insert("title", "1984")?;
    book.insert("author", "George Orwell")?;
    book.insert("price", 10)?;
    book.insert("available", true)?;
    book.insert("isbn", "isbn-1984")?;

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
    book.insert("isbn", "isbn-bnw")?;

    let book = books.insert_container(3, LoroMap::new())?;
    book.insert("title", "Fahrenheit 451")?;
    book.insert("author", "Ray Bradbury")?;
    book.insert("price", LoroValue::Null)?;
    book.insert("available", true)?;
    book.insert("isbn", "isbn-f451")?;

    let book = books.insert_container(4, LoroMap::new())?;
    book.insert("title", "Pride and Prejudice")?;
    book.insert("author", "Jane Austen")?;
    book.insert("price", 7)?;
    book.insert("available", true)?;

    let featured_authors = store.insert_container("featured_authors", LoroList::new())?;
    featured_authors.push("George Orwell")?;
    featured_authors.push("Jane Austen")?;
    store.insert("featured_author", "George Orwell")?;
    store.insert("min_price", 10)?;
    store.insert("line\nbreak", "split")?;
    store.insert("emoji 😀", "smile")?;

    doc.commit();
    Ok(doc)
}

#[test]
fn jsonpath_functions_and_compound_filters_follow_contract() -> anyhow::Result<()> {
    let doc = build_doc()?;

    assert_eq!(
        titles(doc.jsonpath(
            "$.store.books[?(count(@.title) == 1 && count($.store.books[?(@.available == true)]) == 4 && length(value($.store.featured_authors)) == 2 && length(value($.store.books[0])) == 5 && length(value($.store.featured_author)) == 13 && value($.store.featured_author) == 'George Orwell' && @.author == $.store.featured_author)].title"
        )?),
        vec!["1984", "Animal Farm"]
    );

    assert_eq!(
        titles(doc.jsonpath(
            "$.store.books[?((@.author in ['George Orwell', 'Jane Austen'] && @.available == true && @.price >= 6) || (@.title contains 'World' && !(@.available == true)) || (@.price == null))].title"
        )?),
        vec![
            "1984",
            "Animal Farm",
            "Brave New World",
            "Fahrenheit 451",
            "Pride and Prejudice",
        ]
    );

    assert_eq!(
        titles(doc.jsonpath("$.store.books[-2:].title")?),
        vec!["Fahrenheit 451", "Pride and Prejudice"]
    );
    assert_eq!(
        titles(doc.jsonpath("$.store.books[2,0].title")?),
        vec!["Brave New World", "1984"]
    );
    assert!(doc.jsonpath("$.store.books[0:1:0]")?.is_empty());

    assert_eq!(
        deep_json(doc.jsonpath("$['store']['line\\nbreak']")?),
        json!(["split"])
    );
    assert_eq!(
        deep_json(doc.jsonpath("$['store']['emoji \\uD83D\\uDE00']")?),
        json!(["smile"])
    );

    let snapshot = LoroDoc::from_snapshot(&doc.export(ExportMode::Snapshot)?)?;
    assert_eq!(
        titles(snapshot.jsonpath("$.store.books[?(@.author in $.store.featured_authors)].title")?),
        vec!["1984", "Animal Farm", "Pride and Prejudice"]
    );

    let imported = LoroDoc::new();
    imported.import(&doc.export(ExportMode::all_updates())?)?;
    assert_eq!(
        titles(
            imported.jsonpath(
                "$.store.books[?(@.price == null || @.price >= $.store.min_price)].title"
            )?
        ),
        vec!["1984", "Brave New World", "Fahrenheit 451"]
    );

    Ok(())
}

#[test]
fn jsonpath_function_parser_rejects_bad_arity_and_types() -> anyhow::Result<()> {
    let doc = build_doc()?;

    for invalid in [
        "$.store.books[?count()]",
        "$.store.books[?count(1)]",
        "$.store.books[?length()]",
        "$.store.books[?length($.store.books[*])]",
        "$.store.books[?value('x')]",
        "$.store.books[?count(@.title)]",
        "$.store.books[?match(@.title)]",
        "$.store.books[?search(@.title, 'Farm', 'extra')]",
        "$.store.books[?foo(@.title)]",
        "$.store.books[?match(@.title, '1984') == true]",
        "$.store.books[?search(@.title, 'Farm') == true]",
        "$.store.books[?(@.title == $.store.books[*].title)]",
    ] {
        assert!(
            doc.jsonpath(invalid).is_err(),
            "{invalid} should be rejected"
        );
    }

    Ok(())
}

#[test]
fn jsonpath_invalid_escapes_indices_and_syntax_are_rejected() -> anyhow::Result<()> {
    let doc = build_doc()?;

    for invalid in [
        "store.book",
        "$.store.books[",
        "$.store.books[?(@.price <)]",
        "$.store.books[9223372036854775808]",
        "$.store['bad\\q']",
        "$.store['\\uD800']",
    ] {
        assert!(
            doc.jsonpath(invalid).is_err(),
            "{invalid} should be rejected"
        );
    }

    Ok(())
}

#[test]
fn jsonpath_subscribe_jsonpath_tracks_function_based_filters() -> anyhow::Result<()> {
    let doc = build_doc()?;
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_ref = Arc::clone(&hits);
    let sub = doc.subscribe_jsonpath(
        "$.store.books[?(count(@.title) == 1 && @.author == $.store.featured_author && length(value($.store.featured_authors)) == 2)].title",
        Arc::new(move || {
            hits_ref.fetch_add(1, Ordering::SeqCst);
        }),
    )?;

    let books = doc
        .get_map("store")
        .get("books")
        .unwrap()
        .into_container()
        .unwrap()
        .into_list()
        .unwrap();

    let new_book = books.insert_container(books.len(), LoroMap::new())?;
    new_book.insert("title", "Dune")?;
    new_book.insert("author", "George Orwell")?;
    new_book.insert("price", 11)?;
    new_book.insert("available", true)?;
    doc.commit();
    assert!(hits.load(Ordering::SeqCst) >= 1);

    sub.unsubscribe();
    let hits_after_unsubscribe = hits.load(Ordering::SeqCst);
    new_book.insert("title", "Dune Messiah")?;
    doc.commit();
    assert_eq!(hits.load(Ordering::SeqCst), hits_after_unsubscribe);

    Ok(())
}
