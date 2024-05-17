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
