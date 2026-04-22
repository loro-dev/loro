#![cfg(feature = "jsonpath")]

use loro_internal::{
    handler::ValueOrHandler, jsonpath::jsonpath_impl::evaluate_jsonpath, loro_value, HandlerTrait,
    LoroValue, ToJson,
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use std::error::Error;

fn query_json(root: &LoroValue, path: &str) -> Result<Value, Box<dyn Error>> {
    Ok(Value::Array(
        evaluate_jsonpath(root, path)?
            .into_iter()
            .map(|value| match value {
                ValueOrHandler::Value(value) => value.to_json_value(),
                ValueOrHandler::Handler(handler) => handler.get_deep_value().to_json_value(),
            })
            .collect(),
    ))
}

fn root_value() -> LoroValue {
    loro_value!({
        "store": {
            "books": [
                {
                    "title": "1984",
                    "author": "George Orwell",
                    "price": 10,
                    "rating": 4.5,
                    "available": true,
                    "tags": ["classic", "dystopia"],
                    "meta": {"pages": 328, "edition": "paperback"},
                    "note": null,
                    "zero": 0,
                    "empty": "",
                    "empty_list": [],
                    "empty_map": {}
                },
                {
                    "title": "Animal Farm",
                    "author": "George Orwell",
                    "price": 6,
                    "rating": 4.0,
                    "available": true,
                    "tags": ["classic"],
                    "meta": {"pages": 112},
                    "note": "satire",
                    "zero": 0,
                    "empty": "",
                    "empty_list": [],
                    "empty_map": {}
                },
                {
                    "title": "Brave New World",
                    "author": "Aldous Huxley",
                    "price": 12,
                    "rating": 4.2,
                    "available": false,
                    "tags": [],
                    "meta": {},
                    "note": null,
                    "zero": 0,
                    "empty": "",
                    "empty_list": [],
                    "empty_map": {}
                }
            ],
            "featured_authors": ["George Orwell"],
            "min_price": 9,
            "empty": "",
            "truth": true,
            "nothing": null
        },
        "numbers": [0, 1, 2, 3]
    })
}

#[test]
fn jsonpath_evaluates_raw_loro_values_with_comparisons_and_truthiness() -> Result<(), Box<dyn Error>>
{
    let root = root_value();

    assert_eq!(query_json(&root, "$")?, json!([root.to_json_value()]));
    assert_eq!(
        query_json(&root, "$.store.books[-1].title")?,
        json!(["Brave New World"])
    );
    assert_eq!(query_json(&root, "$.store.books[-99]")?, json!([]));

    assert_eq!(
        query_json(&root, "$.store.books[?(@.price != 10)].title")?,
        json!(["Animal Farm", "Brave New World"])
    );
    assert_eq!(
        query_json(
            &root,
            "$.store.books[?(@.price <= $.store.min_price)].title"
        )?,
        json!(["Animal Farm"])
    );
    assert_eq!(
        query_json(&root, "$.store.books[?(@.price > 10)].title")?,
        json!(["Brave New World"])
    );
    assert_eq!(
        query_json(&root, "$.store.books[?(@.rating == 4.5)].title")?,
        json!(["1984"])
    );
    assert_eq!(
        query_json(&root, "$.store.books[?(@.rating >= 4.2)].title")?,
        json!(["1984", "Brave New World"])
    );
    assert_eq!(
        query_json(
            &root,
            "$.store.books[?(@.title >= 'Brave New World')].title"
        )?,
        json!(["Brave New World"])
    );
    assert_eq!(
        query_json(&root, "$.store.books[?(@.title < 'Brave New World')].title")?,
        json!(["1984", "Animal Farm"])
    );
    assert_eq!(
        query_json(&root, "$.store.books[?(@.available != false)].title")?,
        json!(["1984", "Animal Farm"])
    );
    assert_eq!(
        query_json(
            &root,
            "$.store.books[?(@.author in $.store.featured_authors)].title"
        )?,
        json!(["1984", "Animal Farm"])
    );
    assert_eq!(
        query_json(&root, "$.store.books[?(@.author in 'George Orwell')].title")?,
        json!([])
    );

    assert_eq!(
        query_json(&root, "$.store.books[?(count(@.title) == 1)].title")?,
        json!(["1984", "Animal Farm", "Brave New World"])
    );
    assert!(evaluate_jsonpath(&root, "$.store.books[?(count(1) == 0)].title").is_err());
    assert_eq!(
        query_json(&root, "$.store.books[?(length(value(@.tags)) > 0)].title")?,
        json!(["1984", "Animal Farm"])
    );
    assert_eq!(
        query_json(&root, "$.store.books[?(length(value(@.meta)) >= 2)].title")?,
        json!(["1984"])
    );
    assert_eq!(
        query_json(&root, "$.store.books[?(length(value(@.price)) == 0)].title")?,
        json!(["1984", "Animal Farm", "Brave New World"])
    );
    assert_eq!(
        query_json(&root, "$.store.books[?(value(@.tags[*]) == null)].title")?,
        json!(["1984", "Brave New World"])
    );

    assert!(evaluate_jsonpath(&root, "$.store.books[?(value(@.title))].title").is_err());
    assert_eq!(
        query_json(&root, "$.store.books[?(@.missing)].title")?,
        json!([])
    );
    assert_eq!(query_json(&root, "$.numbers[?(@ >= 2)]")?, json!([2, 3]));

    Ok(())
}
