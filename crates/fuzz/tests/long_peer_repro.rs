#![allow(deprecated)]

use fuzz::{
    actions::{ActionWrapper::Generic, GenericAction},
    crdt_fuzzer::{test_multi_sites, Action::*, FuzzTarget, FuzzValue::*},
};

#[test]
fn state_only_round_trip_with_shallow_deps_does_not_abort() {
    let mut actions = vec![
        Handle {
            site: 5,
            target: 0,
            container: 1,
            action: Generic(GenericAction {
                value: I32(1640433118),
                bool: true,
                key: 0,
                pos: 127,
                length: 5572,
                prop: 10932530838780215782,
            }),
        },
        Handle {
            site: 6,
            target: 2,
            container: 255,
            action: Generic(GenericAction {
                value: I32(-75427523),
                bool: true,
                key: 127,
                pos: 255,
                length: 6464427107123988093,
                prop: 8961679350970528599,
            }),
        },
        Handle {
            site: 11,
            target: 5,
            container: 255,
            action: Generic(GenericAction {
                value: I32(-2115244194),
                bool: true,
                key: 5593,
                pos: 1,
                length: 127,
                prop: 13441469769107750371,
            }),
        },
        Handle {
            site: 3,
            target: 2,
            container: 128,
            action: Generic(GenericAction {
                value: I32(-492295482),
                bool: false,
                key: 255,
                pos: 127,
                length: 3,
                prop: 15291696357131272734,
            }),
        },
        Sync { from: 1, to: 5 },
        Handle {
            site: 10,
            target: 1,
            container: 3,
            action: Generic(GenericAction {
                value: I32(758704948),
                bool: true,
                key: 127,
                pos: 256,
                length: 3,
                prop: 11439594629938706177,
            }),
        },
        Handle {
            site: 8,
            target: 5,
            container: 127,
            action: Generic(GenericAction {
                value: I32(384465392),
                bool: false,
                key: 1,
                pos: 1,
                length: 128,
                prop: 1756241239281040330,
            }),
        },
        Handle {
            site: 4,
            target: 4,
            container: 3,
            action: Generic(GenericAction {
                value: I32(1898508399),
                bool: false,
                key: 3182095529,
                pos: 2125275211552930922,
                length: 2,
                prop: 5249056792262085302,
            }),
        },
        Handle {
            site: 9,
            target: 1,
            container: 2,
            action: Generic(GenericAction {
                value: I32(1769830992),
                bool: true,
                key: 3405228845,
                pos: 5603,
                length: 0,
                prop: 13770125680869519507,
            }),
        },
        Handle {
            site: 1,
            target: 5,
            container: 127,
            action: Generic(GenericAction {
                value: I32(1639017806),
                bool: false,
                key: 256,
                pos: 63,
                length: 5604,
                prop: 7650522914157832242,
            }),
        },
        Handle {
            site: 5,
            target: 2,
            container: 2,
            action: Generic(GenericAction {
                value: I32(1326875017),
                bool: false,
                key: 0,
                pos: 5605,
                length: 32,
                prop: 10425227602370068081,
            }),
        },
        Handle {
            site: 2,
            target: 0,
            container: 2,
            action: Generic(GenericAction {
                value: I32(-198540863),
                bool: false,
                key: 2839136440,
                pos: 255,
                length: 8603975333225844236,
                prop: 1071776543286483368,
            }),
        },
        SyncAll,
        StateOnlyRoundTrip { site: 5 },
    ];
    test_multi_sites(12, vec![FuzzTarget::All], &mut actions);
}
