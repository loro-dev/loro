use loro_common::ContainerType;
use loro_internal::{jsoninit, LoroDoc};
use serde_json::json;

pub fn initialize_without_mappings() {
    let json = json!({
        "root": {
            "list" : [1, 2, 3],
            "map" : {
                "key" : "value"
            }
        }
    });

    let doc = LoroDoc::try_from_json(json, vec![].as_slice()).unwrap();
    dbg!(doc.get_value());
}

pub fn initialize_with_mappings() {
    let json = json!({
        "root": {
            "list" : [1, 2, 3],
            "map" : {
                "key" : "value"
            }
        }
    });

    let mappings = vec![
        // root itself is a LoroMap
        jsoninit::PathMapping::new("$.root", ContainerType::Map),
        // list is a LoroList
        jsoninit::PathMapping::new("$.root.list", ContainerType::MovableList),
        // every item inside the map is a LoroText
        jsoninit::PathMapping::new("$.root.map[*]", ContainerType::Text),
    ];

    let doc = LoroDoc::try_from_json(json, mappings.as_slice()).unwrap();
    dbg!(doc.get_value());
}

pub fn main() {
    initialize_without_mappings();
    initialize_with_mappings();
}
