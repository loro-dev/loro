#![cfg(feature = "jsonpath")]

use loro::{ExportMode, LoroDoc, LoroList, LoroMap, LoroValue, ToJson};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn to_json(values: Vec<loro::ValueOrContainer>) -> Value {
    Value::Array(
        values
            .into_iter()
            .map(|value| value.get_deep_value().to_json_value())
            .collect(),
    )
}

fn sorted_strings(values: Vec<loro::ValueOrContainer>) -> Vec<String> {
    let mut strings: Vec<_> = values
        .into_iter()
        .map(|value| value.as_value().unwrap().as_string().unwrap().to_string())
        .collect();
    strings.sort();
    strings
}

fn insert_book(
    books: &loro::LoroList,
    index: usize,
    title: &str,
    author: &str,
    price: Option<i64>,
    available: bool,
    isbn: Option<&str>,
) -> anyhow::Result<LoroMap> {
    let book = books.insert_container(index, LoroMap::new())?;
    book.insert("title", title)?;
    book.insert("author", author)?;
    book.insert("price", price.map_or(LoroValue::Null, LoroValue::from))?;
    book.insert("available", available)?;
    if let Some(isbn) = isbn {
        book.insert("isbn", isbn)?;
    }

    Ok(book)
}

fn build_catalog() -> anyhow::Result<LoroDoc> {
    let doc = LoroDoc::new();
    doc.set_peer_id(71)?;
    let store = doc.get_map("store");
    let books = store.insert_container("books", LoroList::new())?;

    insert_book(
        &books,
        0,
        "1984",
        "George Orwell",
        Some(10),
        true,
        Some("isbn-1984"),
    )?;
    insert_book(
        &books,
        1,
        "Animal Farm",
        "George Orwell",
        Some(6),
        true,
        None,
    )?;
    insert_book(
        &books,
        2,
        "Brave New World",
        "Aldous Huxley",
        Some(12),
        false,
        Some("isbn-bnw"),
    )?;
    insert_book(
        &books,
        3,
        "Fahrenheit 451",
        "Ray Bradbury",
        None,
        true,
        Some("isbn-f451"),
    )?;
    insert_book(
        &books,
        4,
        "Pride and Prejudice",
        "Jane Austen",
        Some(7),
        true,
        None,
    )?;

    let featured = store.insert_container("featured_authors", LoroList::new())?;
    featured.push("George Orwell")?;
    featured.push("Ray Bradbury")?;
    store.insert("featured_author", "George Orwell")?;
    store.insert("min_price", 9)?;

    doc.commit();
    Ok(doc)
}

#[test]
fn jsonpath_filters_root_refs_and_roundtrips_state() -> anyhow::Result<()> {
    let doc = build_catalog()?;

    assert_eq!(
        sorted_strings(
            doc.jsonpath("$.store.books[?(@.author in $.store.featured_authors)].title")?
        ),
        vec!["1984", "Animal Farm", "Fahrenheit 451"]
    );
    assert_eq!(
        sorted_strings(
            doc.jsonpath("$.store.books[?(@.price == null || @.price < $.store.min_price)].title")?
        ),
        vec!["Animal Farm", "Fahrenheit 451", "Pride and Prejudice"]
    );
    assert_eq!(
        sorted_strings(doc.jsonpath(
            "$.store.books[?(@.author == $.store.featured_author && @.available == true)].title"
        )?),
        vec!["1984", "Animal Farm"]
    );
    assert_eq!(
        to_json(doc.jsonpath("$.store.books[0,2]['title','author']")?),
        json!(["1984", "George Orwell", "Brave New World", "Aldous Huxley"])
    );
    assert_eq!(
        sorted_strings(doc.jsonpath("$..[?(!@.isbn)].title")?),
        vec!["Animal Farm", "Pride and Prejudice"]
    );

    let snapshot = LoroDoc::from_snapshot(&doc.export(ExportMode::Snapshot)?)?;
    assert_eq!(
        sorted_strings(snapshot.jsonpath("$.store.books[?(@.price >= $.store.min_price)].title")?),
        vec!["1984", "Brave New World"]
    );

    let imported = LoroDoc::new();
    imported.import(&doc.export(ExportMode::all_updates())?)?;
    assert_eq!(
        sorted_strings(imported.jsonpath("$..[?(@.author contains 'Orwell')].title")?),
        vec!["1984", "Animal Farm"]
    );

    Ok(())
}

#[test]
fn jsonpath_quoted_keys_and_invalid_queries_follow_contract() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let store = doc.get_map("store");
    let dashed = store.insert_container("book-with-dash", LoroMap::new())?;
    dashed.insert("price-$10", "cheap")?;
    dashed.insert("spaced key", "value")?;
    doc.commit();

    assert_eq!(
        to_json(doc.jsonpath("$['store']['book-with-dash']['price-$10']")?),
        json!(["cheap"])
    );
    assert_eq!(
        to_json(doc.jsonpath("$.store['book-with-dash']['spaced key']")?),
        json!(["value"])
    );
    assert!(doc.jsonpath("$.store.missing").unwrap().is_empty());
    assert!(doc.jsonpath("$.store.books[0:1:0]")?.is_empty());

    for invalid in [
        "store.book",
        "$.store.books[",
        "$.store.books[?(@.price <)]",
    ] {
        assert!(
            doc.jsonpath(invalid).is_err(),
            "{invalid} should be rejected"
        );
    }

    Ok(())
}

#[test]
fn jsonpath_subscriptions_have_no_false_negative_notifications() -> anyhow::Result<()> {
    let doc = build_catalog()?;
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_ref = Arc::clone(&hits);
    let sub = doc.subscribe_jsonpath(
        "$.store.books[?(@.available == true)].title",
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
    new_book.insert("author", "Frank Herbert")?;
    new_book.insert("price", 11)?;
    new_book.insert("available", true)?;
    doc.commit();
    assert!(hits.load(Ordering::SeqCst) >= 1);
    assert_eq!(
        to_json(doc.jsonpath("$.store.books[-1].title")?),
        json!(["Dune"])
    );

    sub.unsubscribe();
    let hits_after_unsubscribe = hits.load(Ordering::SeqCst);
    new_book.insert("title", "Dune Messiah")?;
    doc.commit();
    assert_eq!(hits.load(Ordering::SeqCst), hits_after_unsubscribe);

    Ok(())
}
