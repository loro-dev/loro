use loro_common::ContainerType;
use loro_internal::{jsoninit, LoroDoc};
use serde_json::json;

pub fn main() {
    let json = json!({
        "nodes": {
            "a": {
                "children": ["b", "c"]
            },
        },
        "edges": [
            {"from": "a", "to": "b"},
        ],
        "ids": ["a", "b", "c"]
    });

    let mappings = vec![
        jsoninit::PathMapping::new("nodes", "$.nodes", ContainerType::Map),
        jsoninit::PathMapping::new("edges", "$.edges", ContainerType::MovableList),
        jsoninit::PathMapping::new("ids", "$.ids", ContainerType::List),
    ];

    let mut doc = LoroDoc::try_from_json(json, mappings).unwrap();

    dbg!(doc.get_value());
}
