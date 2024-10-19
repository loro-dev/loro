use loro::json::redact;
use loro::{LoroDoc, LoroList, LoroMovableList, LoroTree, LoroValue};
use loro_internal::version::VersionRange;

#[test]
fn redact_text_doc() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    //              |-----------------------| <- 24 ops
    text.insert(0, "Hello, world! This is a secret message.")
        .unwrap();

    let mut json = doc.export_json_updates(&Default::default(), &doc.oplog_vv());
    let mut range = VersionRange::new();
    range.insert(1, 24, 30);
    redact(&mut json, range).unwrap();
    let redacted_doc = LoroDoc::new();
    redacted_doc.import_json_updates(json).unwrap();
    let redacted_text = redacted_doc.get_text("text");
    assert_eq!(
        redacted_text.to_string(),
        "Hello, world! This is a ������ message."
    );
    assert_ne!(text.to_string(), redacted_text.to_string());
}

#[test]
fn redact_map_list_insertions() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let map = doc.get_map("map");
    let list = doc.get_list("list");

    // Insert into map
    map.insert("key1", "sensitive data").unwrap();
    map.insert("key2", 42).unwrap();

    // Insert into list
    list.insert(0, "secret info").unwrap();
    list.insert(1, true).unwrap();

    let mut json = doc.export_json_updates(&Default::default(), &doc.oplog_vv());
    let mut range = VersionRange::new();
    range.insert(1, 0, 5); // Redact all operations
    redact(&mut json, range).unwrap();

    let s = serde_json::to_string_pretty(&json).unwrap();
    let expected = r#"{
  "schema_version": 1,
  "start_version": {},
  "peers": [
    "1"
  ],
  "changes": [
    {
      "id": "0@0",
      "timestamp": 0,
      "deps": [],
      "lamport": 0,
      "msg": null,
      "ops": [
        {
          "container": "cid:root-map:Map",
          "content": {
            "type": "insert",
            "key": "key1",
            "value": null
          },
          "counter": 0
        },
        {
          "container": "cid:root-map:Map",
          "content": {
            "type": "insert",
            "key": "key2",
            "value": null
          },
          "counter": 1
        },
        {
          "container": "cid:root-list:List",
          "content": {
            "type": "insert",
            "pos": 0,
            "value": [
              null,
              null
            ]
          },
          "counter": 2
        }
      ]
    }
  ]
}"#;
    assert_eq!(s, expected);
    let redacted_doc = LoroDoc::new();
    redacted_doc.import_json_updates(json).unwrap();

    let redacted_map = redacted_doc.get_map("map");
    let redacted_list = redacted_doc.get_list("list");

    // Check map values
    assert_eq!(
        redacted_map.get("key1").unwrap().into_value().unwrap(),
        LoroValue::Null
    );
    assert_eq!(
        redacted_map.get("key2").unwrap().into_value().unwrap(),
        LoroValue::Null
    );

    // Check list values
    assert_eq!(
        redacted_list.get(0).unwrap().into_value().unwrap(),
        LoroValue::Null
    );
    assert_eq!(
        redacted_list.get(1).unwrap().into_value().unwrap(),
        LoroValue::Null
    );
}

#[test]
fn redact_movable_list() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let list = doc.get_movable_list("movable_list");
    list.insert(0, "sensitive data 1").unwrap();
    list.insert(1, "sensitive data 2").unwrap();
    list.set(0, "updated sensitive data").unwrap();

    let mut json = doc.export_json_updates(&Default::default(), &doc.oplog_vv());
    let mut range = VersionRange::new();
    range.insert(1, 0, 3);
    redact(&mut json, range).unwrap();
    let redacted_json = serde_json::to_string_pretty(&json).unwrap();
    assert_eq!(
        redacted_json,
        r#"{
  "schema_version": 1,
  "start_version": {},
  "peers": [
    "1"
  ],
  "changes": [
    {
      "id": "0@0",
      "timestamp": 0,
      "deps": [],
      "lamport": 0,
      "msg": null,
      "ops": [
        {
          "container": "cid:root-movable_list:MovableList",
          "content": {
            "type": "insert",
            "pos": 0,
            "value": [
              null,
              null
            ]
          },
          "counter": 0
        },
        {
          "container": "cid:root-movable_list:MovableList",
          "content": {
            "type": "set",
            "elem_id": "L0@0",
            "value": null
          },
          "counter": 2
        }
      ]
    }
  ]
}"#
    );

    // Create a new document from the redacted JSON
    let redacted_doc = LoroDoc::new();
    redacted_doc.import_json_updates(&redacted_json).unwrap();

    let redacted_list = redacted_doc.get_movable_list("movable_list");

    // Check that the insert operations were redacted
    assert_eq!(
        redacted_list.get(0).unwrap().into_value().unwrap(),
        LoroValue::Null
    );
    assert_eq!(
        redacted_list.get(1).unwrap().into_value().unwrap(),
        LoroValue::Null
    );

    // Check that the set operation was redacted
    // The set operation should have replaced the first null value with another null
    assert_eq!(
        redacted_list.get(0).unwrap().into_value().unwrap(),
        LoroValue::Null
    );

    // Verify the list length is correct
    assert_eq!(redacted_list.len(), 2);

    // Optionally, you can print the redacted JSON to inspect it
    // println!("{}", redacted_json);
}

#[test]
fn redact_should_keep_parent_child_relationship() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let map = doc.get_map("map");
    let list = map.insert_container("list", LoroList::new()).unwrap();
    let tree = list.insert_container(0, LoroTree::new()).unwrap();
    let node = tree.create(None).unwrap();
    let sub_map = tree.get_meta(node).unwrap();
    let _m = sub_map
        .insert_container("ll", LoroMovableList::new())
        .unwrap();
    let mut json = doc.export_json_updates(&Default::default(), &doc.oplog_vv());
    let mut range = VersionRange::new();
    range.insert(1, 0, 100);
    redact(&mut json, range).unwrap();
    let redacted_json = serde_json::to_string_pretty(&json).unwrap();
    pretty_assertions::assert_eq!(
        redacted_json,
        r#"{
  "schema_version": 1,
  "start_version": {},
  "peers": [
    "1"
  ],
  "changes": [
    {
      "id": "0@0",
      "timestamp": 0,
      "deps": [],
      "lamport": 0,
      "msg": null,
      "ops": [
        {
          "container": "cid:root-map:Map",
          "content": {
            "type": "insert",
            "key": "list",
            "value": "🦜:cid:0@0:List"
          },
          "counter": 0
        },
        {
          "container": "cid:0@0:List",
          "content": {
            "type": "insert",
            "pos": 0,
            "value": [
              "🦜:cid:1@0:Tree"
            ]
          },
          "counter": 1
        },
        {
          "container": "cid:1@0:Tree",
          "content": {
            "type": "create",
            "target": "2@0",
            "parent": null,
            "fractional_index": "80"
          },
          "counter": 2
        },
        {
          "container": "cid:2@0:Map",
          "content": {
            "type": "insert",
            "key": "ll",
            "value": "🦜:cid:3@0:MovableList"
          },
          "counter": 3
        }
      ]
    }
  ]
}"#
    );
    let new_doc = LoroDoc::new();
    new_doc.import_json_updates(&redacted_json).unwrap();
    assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());
}
