use std::sync::Arc;

use loro::{LoroDoc, LoroError, ToJson};
use serde_json::json;
use tracing::debug_span;

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

#[test]
fn conflict_moves() -> Result<(), LoroError> {
    let doc1 = LoroDoc::new();
    doc1.set_peer_id(1)?;
    let list1 = doc1.get_movable_list("list");
    list1.insert(0, 1)?;
    list1.insert(1, 2)?;
    list1.insert(2, 3)?;
    assert_eq!(
        doc1.get_deep_value().to_json_value(),
        json!({
            "list": [1, 2, 3]
        })
    );

    // 0@1, 1@1, 2@1
    let doc2 = LoroDoc::new();
    doc2.import(&doc1.export_from(&Default::default()))?;
    doc2.set_peer_id(2)?;
    let list2 = doc2.get_movable_list("list");
    // [0@1], 1@1, 2@1, 3@1
    list1.mov(0, 2)?;
    list1.log_internal_state();
    // [0@1], 1@1, 0@2, 2@1
    list2.mov(0, 1)?;
    list2.log_internal_state();
    debug_span!("doc1 import").in_scope(|| {
        doc1.import(&doc2.export_from(&Default::default())).unwrap();
    });
    debug_span!("doc2 import").in_scope(|| {
        doc2.import(&doc1.export_from(&Default::default())).unwrap();
    });
    // [0@1], 1@1, 0@2, 2@1, 3@1
    //   -     2    1    3   (1)
    list1.log_internal_state();
    list2.log_internal_state();
    assert_eq!(doc1.get_deep_value(), doc2.get_deep_value());
    assert_eq!(
        doc1.get_deep_value().to_json_value(),
        json!({
            "list": [2, 1, 3]
        })
    );

    Ok(())
}

#[test]
fn movable_list_event() -> Result<(), LoroError> {
    let doc1 = LoroDoc::new();
    doc1.set_peer_id(1)?;
    doc1.subscribe_root(Arc::new(|e| {
        dbg!(e);
    }));

    let list1 = doc1.get_movable_list("list");
    let _ = list1.insert_container(0, loro_internal::ContainerType::List);
    let _ = list1.insert_container(1, loro_internal::ContainerType::MovableList);
    doc1.commit();
    Ok(())
}
