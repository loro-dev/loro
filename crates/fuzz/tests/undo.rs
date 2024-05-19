use fuzz::{
    actions::{ActionWrapper::*, GenericAction},
    crdt_fuzzer::{test_multi_sites, Action::*, FuzzTarget, FuzzValue::*},
};

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

#[test]
fn undo_tree() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Tree],
        &mut [
            Handle {
                site: 117,
                target: 117,
                container: 117,
                action: Generic(GenericAction {
                    value: I32(1970632053),
                    bool: true,
                    key: 4143972608,
                    pos: 8463800222054970843,
                    length: 17798226830628844917,
                    prop: 8463754637612613851,
                }),
            },
            Undo {
                site: 117,
                op_len: 1970632053,
            },
            Undo {
                site: 117,
                op_len: 1970632053,
            },
            Checkout {
                site: 117,
                to: 1970632053,
            },
            Undo {
                site: 117,
                op_len: 678786421,
            },
            Undo {
                site: 117,
                op_len: 762672501,
            },
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
    );
}
