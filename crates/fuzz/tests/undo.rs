use fuzz::{
    actions::{ActionWrapper::*, GenericAction},
    crdt_fuzzer::{test_multi_sites, Action::*, FuzzTarget, FuzzValue::*},
};

// #[ctor::ctor]
// fn init() {
//     dev_utils::setup_test_log();
// }

#[test]
fn undo_tree_with_map() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Tree],
        &mut [
            Handle {
                site: 174,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(117440512),
                    bool: true,
                    key: 1275068415,
                    pos: 18446743068687204667,
                    length: 46161896180416511,
                    prop: 18446463698227691775,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-12976128),
                    bool: true,
                    key: 131071,
                    pos: 3399988123389597184,
                    length: 3400000218017509167,
                    prop: 3399988123389603631,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(791621423),
                    bool: true,
                    key: 791621423,
                    pos: 18372433783001394991,
                    length: 13281205459693609,
                    prop: 18446744069425331619,
                }),
            },
            SyncAll,
            SyncAllUndo {
                site: 149,
                op_len: 65533,
            },
        ],
    );
}
