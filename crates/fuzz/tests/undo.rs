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
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-13959169),
                    bool: true,
                    key: 4294967295,
                    pos: 11816882472266760191,
                    length: 72057589743025920,
                    prop: 10778762716798463743,
                }),
            },
            SyncAllUndo {
                site: 65,
                op_len: 65327,
            },
        ],
    );
}
