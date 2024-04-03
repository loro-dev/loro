use fuzz::{
    actions::{ActionWrapper::*, GenericAction},
    crdt_fuzzer::{test_multi_sites, Action, Action::*, FuzzTarget, FuzzValue::*},
};
use loro::ContainerType::*;

fn test_actions(mut actions: Vec<Action>) {
    test_multi_sites(5, vec![FuzzTarget::Tree], &mut actions)
}

#[ctor::ctor]
fn init_color_backtrace() {
    color_backtrace::install();
    use tracing_subscriber::{prelude::*, registry::Registry};
    if option_env!("DEBUG").is_some() {
        tracing::subscriber::set_global_default(
            Registry::default().with(tracing_subscriber::fmt::Layer::default()),
        )
        .unwrap();
    }
}

#[test]
fn tree_same_move() {
    test_actions(vec![
        Handle {
            site: 51,
            target: 51,
            container: 51,
            action: Generic(GenericAction {
                value: I32(858993459),
                bool: true,
                key: 868562739,
                pos: 3689348814741910323,
                length: 3689348814741910323,
                prop: 3689348814741910323,
            }),
        },
        Handle {
            site: 51,
            target: 51,
            container: 51,
            action: Generic(GenericAction {
                value: I32(858993459),
                bool: true,
                key: 858993459,
                pos: 3689348814741910323,
                length: 15506794236962091827,
                prop: 3689348814742553055,
            }),
        },
        Handle {
            site: 51,
            target: 197,
            container: 51,
            action: Generic(GenericAction {
                value: I32(858993459),
                bool: true,
                key: 858993459,
                pos: 3689348814741910323,
                length: 3906369333172056883,
                prop: 18446744066029139510,
            }),
        },
        Handle {
            site: 51,
            target: 51,
            container: 51,
            action: Generic(GenericAction {
                value: I32(858993459),
                bool: true,
                key: 858993459,
                pos: 67078248936243,
                length: 42099763356696573,
                prop: 8226,
            }),
        },
    ])
}

#[test]
fn tree() {
    test_actions(vec![
        Handle {
            site: 48,
            target: 91,
            container: 91,
            action: Generic(GenericAction {
                value: Container(Tree),
                bool: true,
                key: 4294967295,
                pos: 18444210798935932927,
                length: 690624182933323867,
                prop: 17800764538523027721,
            }),
        },
        Handle {
            site: 9,
            target: 151,
            container: 149,
            action: Generic(GenericAction {
                value: I32(151324937),
                bool: true,
                key: 4042321929,
                pos: 651061555542749424,
                length: 2543209201338633,
                prop: 11068046444225730836,
            }),
        },
        SyncAll,
        Handle {
            site: 9,
            target: 9,
            container: 9,
            action: Generic(GenericAction {
                value: I32(218695945),
                bool: true,
                key: 151587081,
                pos: 651061555543345417,
                length: 102185350956910857,
                prop: 127186009683460245,
            }),
        },
        Handle {
            site: 91,
            target: 35,
            container: 91,
            action: Generic(GenericAction {
                value: I32(0),
                bool: false,
                key: 0,
                pos: 0,
                length: 0,
                prop: 0,
            }),
        },
    ])
}

#[test]
fn tree_meta() {
    test_actions(vec![
        Handle {
            site: 192,
            target: 255,
            container: 255,
            action: Generic(GenericAction {
                value: Container(List),
                bool: true,
                key: 4294967073,
                pos: 10778686051598729729,
                length: 18446514557159839127,
                prop: 18446743678572560383,
            }),
        },
        Handle {
            site: 189,
            target: 63,
            container: 255,
            action: Generic(GenericAction {
                value: Container(Tree),
                bool: true,
                key: 808976897,
                pos: 14974299229237936383,
                length: 144114232942526463,
                prop: 14925493210863108863,
            }),
        },
        SyncAll,
        Handle {
            site: 34,
            target: 247,
            container: 207,
            action: Generic(GenericAction {
                value: Container(Tree),
                bool: true,
                key: 3680174080,
                pos: 11429747308408484319,
                length: 11429747308416114334,
                prop: 10922800942116175874,
            }),
        },
        Handle {
            site: 255,
            target: 255,
            container: 219,
            action: Generic(GenericAction {
                value: Container(Tree),
                bool: true,
                key: 4294943487,
                pos: 4313092040194523029,
                length: 15806468754477883942,
                prop: 4313092405270512443,
            }),
        },
        Handle {
            site: 247,
            target: 255,
            container: 255,
            action: Generic(GenericAction {
                value: Container(Tree),
                bool: true,
                key: 573518815,
                pos: 247,
                length: 0,
                prop: 0,
            }),
        },
    ])
}

#[test]
fn left_right_same_position() {
    test_actions(vec![
        Handle {
            site: 11,
            target: 11,
            container: 11,
            action: Generic(GenericAction {
                value: I32(957025035),
                bool: true,
                key: 3659596255,
                pos: 18446627069606493975,
                length: 18446744073709551615,
                prop: 18446744073709551615,
            }),
        },
        Handle {
            site: 2,
            target: 255,
            container: 191,
            action: Generic(GenericAction {
                value: Container(Map),
                bool: true,
                key: 4294377471,
                pos: 9104926049750614015,
                length: 327616501915904,
                prop: 18444492273895866112,
            }),
        },
        SyncAll,
        Handle {
            site: 44,
            target: 255,
            container: 0,
            action: Generic(GenericAction {
                value: Container(Text),
                bool: false,
                key: 4136983551,
                pos: 18446744073709551615,
                length: 12826533213727883263,
                prop: 18446744072635744768,
            }),
        },
        SyncAll,
        SyncAll,
        Handle {
            site: 91,
            target: 126,
            container: 0,
            action: Generic(GenericAction {
                value: Container(Text),
                bool: true,
                key: 4294967295,
                pos: 18446744073709551615,
                length: 18446744073709551615,
                prop: 18446744069649465343,
            }),
        },
        SyncAll,
        SyncAll,
        Handle {
            site: 45,
            target: 255,
            container: 255,
            action: Generic(GenericAction {
                value: I32(0),
                bool: false,
                key: 0,
                pos: 0,
                length: 0,
                prop: 0,
            }),
        },
    ])
}

#[test]
fn meta() {
    test_actions(vec![
        Handle {
            site: 131,
            target: 183,
            container: 129,
            action: Generic(GenericAction {
                value: Container(Text),
                bool: true,
                key: 522133279,
                pos: 2242545357980385567,
                length: 18446744073709551615,
                prop: 2242545357980377087,
            }),
        },
        Handle {
            site: 31,
            target: 31,
            container: 159,
            action: Generic(GenericAction {
                value: I32(522133279),
                bool: true,
                key: 4294967295,
                pos: 2242545357980434431,
                length: 2242545357980376863,
                prop: 2242545357980376991,
            }),
        },
        Handle {
            site: 31,
            target: 31,
            container: 31,
            action: Generic(GenericAction {
                value: Container(Tree),
                bool: true,
                key: 4294967295,
                pos: 18446734178104901631,
                length: 6196830562867428351,
                prop: 10416984401456865055,
            }),
        },
        Sync { from: 31, to: 31 },
        Handle {
            site: 31,
            target: 31,
            container: 31,
            action: Generic(GenericAction {
                value: I32(-57569),
                bool: true,
                key: 4294967295,
                pos: 2242545357980434431,
                length: 2242545357980376863,
                prop: 18391046246847422367,
            }),
        },
        Handle {
            site: 47,
            target: 147,
            container: 47,
            action: Generic(GenericAction {
                value: I32(791621377),
                bool: true,
                key: 791621423,
                pos: 3399988123389603631,
                length: 18415500351294668799,
                prop: 6196831041463842363,
            }),
        },
        Handle {
            site: 31,
            target: 31,
            container: 129,
            action: Generic(GenericAction {
                value: Container(List),
                bool: true,
                key: 2172748161,
                pos: 9331882296111890817,
                length: 9331882296111890817,
                prop: 9331882296111890817,
            }),
        },
        Handle {
            site: 31,
            target: 31,
            container: 31,
            action: Generic(GenericAction {
                value: Container(Tree),
                bool: true,
                key: 522190847,
                pos: 2242545357980376863,
                length: 2242545907736190751,
                prop: 3399989020233178911,
            }),
        },
        Handle {
            site: 47,
            target: 47,
            container: 47,
            action: Generic(GenericAction {
                value: I32(791621423),
                bool: true,
                key: 522133295,
                pos: 2242545357980376863,
                length: 6196831041471119135,
                prop: 563538504058,
            }),
        },
    ])
}

#[test]
fn tree2() {
    test_actions(vec![
        Handle {
            site: 23,
            target: 255,
            container: 112,
            action: Generic(GenericAction {
                value: I32(0),
                bool: false,
                key: 524288,
                pos: 0,
                length: 11140386617070441728,
                prop: 230695578868378,
            }),
        },
        SyncAll,
        Handle {
            site: 45,
            target: 45,
            container: 45,
            action: Generic(GenericAction {
                value: I32(-1792201427),
                bool: false,
                key: 8280884,
                pos: 4035225660500335393,
                length: 1975674142468,
                prop: 18446744073709551615,
            }),
        },
        Handle {
            site: 9,
            target: 56,
            container: 9,
            action: Generic(GenericAction {
                value: Container(Text),
                bool: true,
                key: 16844041,
                pos: 88384250654424831,
                length: 72340172838076673,
                prop: 72340172838076673,
            }),
        },
        Handle {
            site: 1,
            target: 1,
            container: 1,
            action: Generic(GenericAction {
                value: I32(16843009),
                bool: true,
                key: 1459683585,
                pos: 361700864190404439,
                length: 361700864190383365,
                prop: 361700864190383365,
            }),
        },
        Handle {
            site: 1,
            target: 1,
            container: 1,
            action: Generic(GenericAction {
                value: I32(16843009),
                bool: true,
                key: 1465341783,
                pos: 6293595036912670551,
                length: 361700864190383365,
                prop: 361700864190383365,
            }),
        },
        Handle {
            site: 5,
            target: 5,
            container: 5,
            action: Generic(GenericAction {
                value: I32(16843009),
                bool: true,
                key: 16843009,
                pos: 130551402805920001,
                length: 18374969075786385665,
                prop: 72402845017046782,
            }),
        },
        Handle {
            site: 1,
            target: 1,
            container: 1,
            action: Generic(GenericAction {
                value: I32(16843009),
                bool: true,
                key: 16843009,
                pos: 72340172838076673,
                length: 288231479975149825,
                prop: 72340172838010880,
            }),
        },
        Handle {
            site: 9,
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
    ])
}
