use fuzz::{
    actions::{ActionWrapper::*, GenericAction},
    crdt_fuzzer::{test_multi_sites, Action::*, FuzzTarget, FuzzValue::*},
};

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

#[test]
fn counter() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Counter],
        &mut [
            Handle {
                site: 8,
                target: 8,
                container: 8,
                action: Generic(GenericAction {
                    value: I32(-13882324),
                    bool: true,
                    key: 589823,
                    pos: 578721382704613384,
                    length: 18446744070155676737,
                    prop: 8,
                }),
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
    )
}
