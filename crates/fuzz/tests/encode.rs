use fuzz::{
    actions::{ActionWrapper::*, GenericAction},
    crdt_fuzzer::{test_multi_sites, Action::*, FuzzTarget, FuzzValue::*},
};
use loro::ContainerType::*;

// #[ctor::ctor]
// fn init() {
//     dev_utils::setup_test_log();
// }

#[test]
fn t() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 61,
                target: 61,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 18446744073709551615,
                    prop: 18385382528786628607,
                }),
            },
            Handle {
                site: 61,
                target: 61,
                container: 61,
                action: Generic(GenericAction {
                    value: I32(1027423549),
                    bool: true,
                    key: 624770365,
                    pos: 4412964684869664573,
                    length: 4412750818000583997,
                    prop: 4412750543122677053,
                }),
            },
            Handle {
                site: 59,
                target: 61,
                container: 61,
                action: Generic(GenericAction {
                    value: I32(708656445),
                    bool: true,
                    key: 474429,
                    pos: 3765064478811488256,
                    length: 4412750543122661376,
                    prop: 217069982108876929,
                }),
            },
            Handle {
                site: 61,
                target: 61,
                container: 61,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 4294967295,
                    pos: 13546827679130402093,
                    length: 9910603678816470750,
                    prop: 4412750542095350528,
                }),
            },
            Handle {
                site: 61,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(1312636221),
                    bool: true,
                    key: 1027439933,
                    pos: 18446743237218352445,
                    length: 272467764503904256,
                    prop: 18375531600403327491,
                }),
            },
            Sync { from: 137, to: 0 },
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
