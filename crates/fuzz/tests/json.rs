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
    let json = doc.export_json(&Default::default());
    let value = doc.get_deep_value();
    // Test1: old version import newer version json
    doc_with_unknown.import_json(&json).unwrap();
    // older version export json format with unknown
    //   {
    //   ...
    //   "changes": [
    //     {
    //       "ops": [
    //         {
    //           "container": "cid:root-counter:Unknown(5)",
    //           "content": {
    //             "type": "unknown",
    //             "value_type": "json_unknown",
    //             "value": "{\"value_type\":\"counter\",\"value\":\"\"}",
    //             "prop": 5
    //           },
    //           "counter": 0
    //         },
    //         ...
    //       ]
    //     }
    //   ]
    // }
    let unknown_json = doc_with_unknown.export_json(&Default::default());
    // older version export snapshot with json-unknown
    let snapshot_with_unknown_json = doc_with_unknown.export_snapshot();
    let doc2_with_unknown = loro_without_counter::LoroDoc::new();

    // Test2: older version import older version snapshot with json-unknown
    doc2_with_unknown
        .import(&snapshot_with_unknown_json)
        .unwrap();

    let new_doc = loro::LoroDoc::new();
    // Test3: newer version import older version json with json-unknown
    new_doc.import_json(&unknown_json).unwrap();
    let new_doc_value = new_doc.get_deep_value();
    let new_doc2 = loro::LoroDoc::new();
    // Test4: newer version import older version snapshot with json-unknown
    new_doc2.import(&snapshot_with_unknown_json).unwrap();
    let snapshot_value = new_doc2.get_deep_value();

    assert_eq!(value, new_doc_value);
    assert_eq!(value, snapshot_value);

    let snapshot_with_counter = doc.export_snapshot();
    let doc3_without_counter = loro_without_counter::LoroDoc::new();
    // Test5: older version import newer version snapshot with counter
    doc3_without_counter.import(&snapshot_with_counter).unwrap();
    let unknown_json_from_snapshot = doc3_without_counter.export_json(&Default::default());
    // {
    //       "container": "cid:root-counter:Unknown(5)",
    //       "content": {
    //         "type": "unknown",
    //         "value_type": "unknown",
    //         "value": "{\"kind\":16,\"data\":[]}",
    //         "prop": 5
    //       },
    //       "counter": 0
    //     }
    // Test6: older version export json with binary unknown
    let _json_with_binary_unknown = doc3_without_counter.export_json(&Default::default());
    let new_doc = loro::LoroDoc::new();
    // Test7: newer version import older version json with binary unknown
    new_doc.import_json(&unknown_json_from_snapshot).unwrap();
    let new_doc_value = new_doc.get_deep_value();
    assert_eq!(value, new_doc_value);
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
