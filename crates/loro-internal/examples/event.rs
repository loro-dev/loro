use std::sync::Arc;

use loro_common::ContainerType;
use loro_internal::{
    delta::DeltaItem,
    event::Diff,
    handler::{Handler, ValueOrHandler},
    LoroDoc, ToJson,
};

fn main() {
    let mut doc = LoroDoc::new();
    doc.start_auto_commit();
    let list = doc.get_list("list");
    doc.subscribe_root(Arc::new(|e| {
        for container_diff in e.events {
            match &container_diff.diff {
                Diff::List(list) => {
                    for item in list.iter() {
                        if let DeltaItem::Insert {
                            insert,
                            attributes: _,
                        } = item
                        {
                            for v in insert {
                                match v {
                                    ValueOrHandler::Handler(h) => {
                                        // You can directly obtain the handler and perform some operations.
                                        if matches!(h, Handler::Map(_)) {
                                            let text = h
                                                .as_map()
                                                .unwrap()
                                                .insert_container("text", ContainerType::Text)
                                                .unwrap();
                                            text.as_text()
                                                .unwrap()
                                                .insert(0, "created from event")
                                                .unwrap();
                                        }
                                    }
                                    ValueOrHandler::Value(value) => {
                                        println!("insert value {:?}", value);
                                    }
                                }
                            }
                        }
                    }
                }
                Diff::Map(map) => {
                    println!("map container updates {:?}", map.updated);
                }
                _ => {}
            }
        }
    }));
    list.insert(0, "abc").unwrap();
    list.insert_container(1, ContainerType::List).unwrap();
    list.insert_container(2, ContainerType::Map).unwrap();
    list.insert_container(3, ContainerType::Text).unwrap();
    list.insert_container(4, ContainerType::Tree).unwrap();
    doc.commit_then_renew();
    assert_eq!(
        doc.get_deep_value().to_json(),
        r#"{"list":["abc",[],{"text":"created from event"},"",[]]}"#
    );
}
