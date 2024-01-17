# Loro 

Loro is a high-performance CRDTs framework offering Rust and JavaScript APIs. 

Designed for local-first software, it enables effortless collaboration in app states. 

PS: Version control is forthcoming. Time travel functionality is already accessible at [https://loro.dev/docs/tutorial/time_travel](https://loro.dev/docs/tutorial/time_travel).

# Examples

## Map/List/Text

```rust
use loro::{LoroDoc, ToJson, LoroValue};
use serde_json::json;

let doc = LoroDoc::new();
let map = doc.get_map("map");
map.insert("key", "value").unwrap();
map.insert("true", true).unwrap();
map.insert("null", LoroValue::Null).unwrap();
map.insert("deleted", LoroValue::Null).unwrap();
map.delete("deleted").unwrap();
let list = map
    .insert_container("list", loro_internal::ContainerType::List).unwrap()
    .into_list()
    .unwrap();
list.insert(0, "List");
list.insert(1, 9);
let text = map
    .insert_container("text", loro_internal::ContainerType::Text).unwrap()
    .into_text()
    .unwrap();
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
```

## Rich Text

```rust
use loro::{ExpandType, LoroDoc, ToJson};
use serde_json::json;

let doc = LoroDoc::new();
let text = doc.get_text("text");
text.insert(0, "Hello world!").unwrap();
text.mark(0..5, "bold", true).unwrap();
assert_eq!(
    text.to_delta().to_json_value(),
    json!([
        { "insert": "Hello", "attributes": {"bold": true} },
        { "insert": " world!" },
    ])
);
text.unmark(3..5, "bold").unwrap();
assert_eq!(
    text.to_delta().to_json_value(),
    json!([
          { "insert": "Hel", "attributes": {"bold": true} },
          { "insert": "lo world!" },
    ])
);
```

## Sync

```rust
use loro::{LoroDoc, ToJson, ExpandType};
use serde_json::json;

let doc = LoroDoc::new();
let text = doc.get_text("text");
text.insert(0, "Hello world!").unwrap();
let bytes = doc.export_from(&Default::default());
let doc_b = LoroDoc::new();
doc_b.import(&bytes).unwrap();
assert_eq!(doc.get_deep_value(), doc_b.get_deep_value());
let text_b = doc_b.get_text("text");
text_b
    .mark(0..5, "bold", true)
    .unwrap();
doc.import(&doc_b.export_from(&doc.oplog_vv())).unwrap();
assert_eq!(
    text.to_delta().to_json_value(),
    json!([
        { "insert": "Hello", "attributes": {"bold": true} },
        { "insert": " world!" },
    ])
);
```

## Save

```rust
use loro::LoroDoc;

let doc = LoroDoc::new();
let text = doc.get_text("text");
text.insert(0, "123").unwrap();
let snapshot = doc.export_snapshot();

let new_doc = LoroDoc::new();
new_doc.import(&snapshot).unwrap();
assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());
```
