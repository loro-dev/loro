use fuzz::{
    actions::{ActionWrapper::*, GenericAction},
    crdt_fuzzer::{test_multi_sites, Action::*, FuzzTarget, FuzzValue::*},
};
use loro::ContainerType::*;

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

#[test]
fn unknown_json() {
    let doc = loro::LoroDoc::new();
    let doc_with_unknown = loro_without_counter::LoroDoc::new();
    let counter = doc.get_counter("counter");
    counter.increment(5).unwrap();
    counter.increment(1).unwrap();
    // json format with counter
    let json = doc.export_json_updates(&Default::default());
    // Test1: old version import newer version json
    if doc_with_unknown
        .import_json_updates(serde_json::to_string(&json).unwrap())
        .is_ok()
    {
        panic!("json schema don't support forward compatibility");
    }

    let snapshot_with_counter = doc.export_snapshot();
    let doc3_without_counter = loro_without_counter::LoroDoc::new();
    // Test2: older version import newer version snapshot with counter
    doc3_without_counter.import(&snapshot_with_counter).unwrap();
    let unknown_json_from_snapshot = doc3_without_counter
        .export_json_updates(&Default::default(), &doc3_without_counter.oplog_vv());
    // {
    //       "container": "cid:root-counter:Unknown(5)",
    //       "content": {
    //         "type": "unknown",
    //         "value_type": "unknown",
    //         "value": {"kind":16,"data":[]},
    //         "prop": 5
    //       },
    //       "counter": 0
    //     }
    // Test3: older version export json with binary unknown
    let _json_with_binary_unknown = doc3_without_counter
        .export_json_updates(&Default::default(), &doc3_without_counter.oplog_vv());
    let new_doc = loro::LoroDoc::new();
    // Test4: newer version import older version json with binary unknown
    if new_doc
        .import_json_updates(serde_json::to_string(&unknown_json_from_snapshot).unwrap())
        .is_ok()
    {
        panic!("json schema don't support forward compatibility");
    }
}

#[test]
fn sub_container() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 0,
                target: 1,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 4293853225,
                    pos: 18446744073709551615,
                    length: 4625477192774582511,
                    prop: 18446744073428216116,
                }),
            },
            Sync { from: 0, to: 1 },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 0,
                    pos: 0,
                    length: 0,
                    prop: 0,
                }),
            },
        ],
    )
}
