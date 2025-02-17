#![allow(unexpected_cfgs)]
#[allow(unused_imports)]
use fuzz::{
    actions::{ActionInner, ActionWrapper::*, GenericAction},
    container::{MapAction, TreeAction, TreeActionInner},
    crdt_fuzzer::{
        test_multi_sites,
        Action::{self, *},
        FuzzTarget,
        FuzzValue::*,
    },
};
use loro::ContainerType::*;

fn test_actions(mut actions: Vec<Action>) {
    test_multi_sites(5, vec![FuzzTarget::Tree], &mut actions)
}

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
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

#[test]
fn delete_node_snapshot_set_parent_container() {
    test_actions(vec![
        Handle {
            site: 3,
            target: 3,
            container: 3,
            action: Generic(GenericAction {
                value: I32(51577603),
                bool: true,
                key: 50529027,
                pos: 217020518514230019,
                length: 217020518514230019,
                prop: 217020905061286659,
            }),
        },
        Handle {
            site: 3,
            target: 3,
            container: 3,
            action: Generic(GenericAction {
                value: I32(50462720),
                bool: true,
                key: 2981167875,
                pos: 12754670997176693169,
                length: 217020522183085391,
                prop: 12804209842084578051,
            }),
        },
        Sync { from: 1, to: 177 },
        Handle {
            site: 3,
            target: 3,
            container: 3,
            action: Generic(GenericAction {
                value: I32(3),
                bool: false,
                key: 0,
                pos: 10634005407187599360,
                length: 287949535011117971,
                prop: 361135706590085891,
            }),
        },
        Sync { from: 3, to: 3 },
        Sync { from: 177, to: 177 },
        Handle {
            site: 3,
            target: 3,
            container: 3,
            action: Generic(GenericAction {
                value: I32(50529027),
                bool: true,
                key: 50529027,
                pos: 12804210592328123141,
                length: 266189226787320285,
                prop: 266189179564786435,
            }),
        },
        Handle {
            site: 3,
            target: 177,
            container: 177,
            action: Generic(GenericAction {
                value: Container(Tree),
                bool: true,
                key: 2147483648,
                pos: 8862802595698180096,
                length: 18446657474297004031,
                prop: 12804210592331923654,
            }),
        },
        Handle {
            site: 177,
            target: 177,
            container: 177,
            action: Generic(GenericAction {
                value: Container(List),
                bool: true,
                key: 2981212593,
                pos: 12803172990249447857,
                length: 217020522758668209,
                prop: 217212583781008133,
            }),
        },
        Sync { from: 177, to: 126 },
        Handle {
            site: 3,
            target: 3,
            container: 3,
            action: Generic(GenericAction {
                value: I32(50529027),
                bool: true,
                key: 50444547,
                pos: 2716147424077349635,
                length: 217020518514286001,
                prop: 6702203981927744259,
            }),
        },
        Handle {
            site: 3,
            target: 3,
            container: 3,
            action: Generic(GenericAction {
                value: I32(50529027),
                bool: true,
                key: 2981212419,
                pos: 12804017078295966129,
                length: 217583468481982897,
                prop: 217020521444913411,
            }),
        },
        Sync { from: 3, to: 3 },
        Handle {
            site: 3,
            target: 3,
            container: 3,
            action: Generic(GenericAction {
                value: I32(-1190984957),
                bool: true,
                key: 632402353,
                pos: 36299749842353,
                length: 0,
                prop: 0,
            }),
        },
    ])
}

#[test]
fn fractional_index_same_parent_move() {
    test_actions(vec![
        Handle {
            site: 3,
            target: 3,
            container: 19,
            action: Generic(GenericAction {
                value: I32(50529027),
                bool: true,
                key: 503513859,
                pos: 216172782113783808,
                length: 226027717768971011,
                prop: 217020518514230019,
            }),
        },
        Handle {
            site: 3,
            target: 3,
            container: 140,
            action: Generic(GenericAction {
                value: Container(List),
                bool: false,
                key: 50529027,
                pos: 217020518514230019,
                length: 217020518514230019,
                prop: 217021268757775107,
            }),
        },
        Handle {
            site: 3,
            target: 3,
            container: 3,
            action: Generic(GenericAction {
                value: I32(587399939),
                bool: true,
                key: 50529027,
                pos: 217020518514230019,
                length: 10736633192992735231,
                prop: 2315413798384378368,
            }),
        },
        SyncAll,
        Handle {
            site: 0,
            target: 255,
            container: 255,
            action: Generic(GenericAction {
                value: I32(14024495),
                bool: false,
                key: 1128333501,
                pos: 18416626253399802824,
                length: 41939973410127871,
                prop: 17807270312518139170,
            }),
        },
        Handle {
            site: 255,
            target: 255,
            container: 255,
            action: Generic(GenericAction {
                value: I32(-256),
                bool: true,
                key: 522133279,
                pos: 217020639247081247,
                length: 217020518514230019,
                prop: 217021268757775107,
            }),
        },
        Handle {
            site: 3,
            target: 3,
            container: 3,
            action: Generic(GenericAction {
                value: I32(587399939),
                bool: true,
                key: 50529027,
                pos: 217020518514230019,
                length: 10736633192992735231,
                prop: 2315413798384378368,
            }),
        },
        SyncAll,
        Handle {
            site: 0,
            target: 255,
            container: 255,
            action: Generic(GenericAction {
                value: I32(14024495),
                bool: false,
                key: 1128333501,
                pos: 18416626253399802824,
                length: 41939973410127871,
                prop: 17807270312518139170,
            }),
        },
        Handle {
            site: 255,
            target: 255,
            container: 255,
            action: Generic(GenericAction {
                value: Container(Tree),
                bool: true,
                key: 4244438273,
                pos: 18445898549283389403,
                length: 562052322033663,
                prop: 9224220851190955042,
            }),
        },
    ])
}

#[test]
fn move_out_first_and_error() {
    // so we don't move the position back to the cache
    test_actions(vec![
        Handle {
            site: 247,
            target: 213,
            container: 149,
            action: Generic(GenericAction {
                value: Container(Tree),
                bool: true,
                key: 6,
                pos: 3272544761136750467,
                length: 9088016791583588226,
                prop: 4683743612450781440,
            }),
        },
        Handle {
            site: 31,
            target: 31,
            container: 126,
            action: Generic(GenericAction {
                value: Container(Tree),
                bool: true,
                key: 3688618971,
                pos: 3689348814741822426,
                length: 59537746179638,
                prop: 3689419196421505024,
            }),
        },
        Handle {
            site: 43,
            target: 51,
            container: 51,
            action: Generic(GenericAction {
                value: I32(16834355),
                bool: false,
                key: 2432761853,
                pos: 72092924448703232,
                length: 15795320375969969920,
                prop: 15842498006749428187,
            }),
        },
        SyncAll,
        Handle {
            site: 51,
            target: 255,
            container: 255,
            action: Generic(GenericAction {
                value: Container(Text),
                bool: false,
                key: 528678912,
                pos: 18411328161714216735,
                length: 4107421532293111583,
                prop: 18446534066988646178,
            }),
        },
        Handle {
            site: 126,
            target: 219,
            container: 219,
            action: Generic(GenericAction {
                value: Container(Tree),
                bool: true,
                key: 3688618971,
                pos: 3906366021785826097,
                length: 908473910,
                prop: 3725377612834813494,
            }),
        },
        Handle {
            site: 51,
            target: 39,
            container: 51,
            action: Generic(GenericAction {
                value: Container(Tree),
                bool: true,
                key: 2197852416,
                pos: 17798226827418927253,
                length: 4169166897523252013,
                prop: 15806469054522195746,
            }),
        },
        SyncAll,
        Handle {
            site: 164,
            target: 219,
            container: 59,
            action: Generic(GenericAction {
                value: I32(-610936018),
                bool: true,
                key: 2508405723,
                pos: 17807940169679920091,
                length: 15817552129580307967,
                prop: 10778686069027887963,
            }),
        },
        Sync { from: 255, to: 255 },
        Sync { from: 163, to: 163 },
        Handle {
            site: 247,
            target: 255,
            container: 255,
            action: Generic(GenericAction {
                value: Container(Text),
                bool: true,
                key: 777771925,
                pos: 2486896337669325275,
                length: 9456393030167035895,
                prop: 1014351739426,
            }),
        },
    ])
}

#[test]
fn clean_children_cache_when_delete() {
    test_actions(vec![
        Handle {
            site: 3,
            target: 1,
            container: 0,
            action: Action(ActionInner::Tree(TreeAction {
                target: (3, 0),
                action: TreeActionInner::Create { index: 0 },
            })),
        },
        Handle {
            site: 3,
            target: 1,
            container: 0,
            action: Action(ActionInner::Tree(TreeAction {
                target: (3, 1),
                action: TreeActionInner::Create { index: 0 },
            })),
        },
        SyncAll,
        Handle {
            site: 1,
            target: 1,
            container: 0,
            action: Action(ActionInner::Tree(TreeAction {
                target: (3, 0),
                action: TreeActionInner::Move {
                    parent: (3, 1),
                    index: 0,
                },
            })),
        },
        SyncAll,
        Handle {
            site: 3,
            target: 1,
            container: 0,
            action: Action(ActionInner::Tree(TreeAction {
                target: (3, 2),
                action: TreeActionInner::Create { index: 0 },
            })),
        },
        SyncAll,
        Handle {
            site: 4,
            target: 1,
            container: 0,
            action: Action(ActionInner::Tree(TreeAction {
                target: (3, 2),
                action: TreeActionInner::Move {
                    parent: (3, 1),
                    index: 0,
                },
            })),
        },
        SyncAll,
        Handle {
            site: 1,
            target: 1,
            container: 0,
            action: Action(ActionInner::Tree(TreeAction {
                target: (3, 1),
                action: TreeActionInner::Delete,
            })),
        },
        SyncAll,
        Handle {
            site: 1,
            target: 1,
            container: 0,
            action: Action(ActionInner::Tree(TreeAction {
                target: (1, 2),
                action: TreeActionInner::Create { index: 0 },
            })),
        },
        Handle {
            site: 1,
            target: 1,
            container: 0,
            action: Action(ActionInner::Tree(TreeAction {
                target: (1, 2),
                action: TreeActionInner::Delete,
            })),
        },
        Handle {
            site: 1,
            target: 1,
            container: 0,
            action: Action(ActionInner::Tree(TreeAction {
                target: (1, 4),
                action: TreeActionInner::Create { index: 0 },
            })),
        },
        Handle {
            site: 1,
            target: 1,
            container: 0,
            action: Action(ActionInner::Tree(TreeAction {
                target: (1, 4),
                action: TreeActionInner::Delete,
            })),
        },
        Handle {
            site: 1,
            target: 1,
            container: 0,
            action: Action(ActionInner::Tree(TreeAction {
                target: (1, 6),
                action: TreeActionInner::Create { index: 0 },
            })),
        },
        Checkout { site: 2, to: 3 },
        Handle {
            site: 1,
            target: 1,
            container: 0,
            action: Action(ActionInner::Tree(TreeAction {
                target: (1, 7),
                action: TreeActionInner::Create { index: 0 },
            })),
        },
        Handle {
            site: 2,
            target: 1,
            container: 0,
            action: Action(ActionInner::Tree(TreeAction {
                target: (2, 0),
                action: TreeActionInner::Create { index: 0 },
            })),
        },
        Handle {
            site: 1,
            target: 1,
            container: 0,
            action: Action(ActionInner::Tree(TreeAction {
                target: (1, 7),
                action: TreeActionInner::Move {
                    parent: (1, 6),
                    index: 0,
                },
            })),
        },
        Handle {
            site: 2,
            target: 1,
            container: 0,
            action: Action(ActionInner::Tree(TreeAction {
                target: (2, 0),
                action: TreeActionInner::Delete,
            })),
        },
        Handle {
            site: 2,
            target: 1,
            container: 0,
            action: Action(ActionInner::Tree(TreeAction {
                target: (2, 3),
                action: TreeActionInner::Create { index: 0 },
            })),
        },
        SyncAll,
        Handle {
            site: 1,
            target: 1,
            container: 0,
            action: Action(ActionInner::Tree(TreeAction {
                target: (2, 2),
                action: TreeActionInner::Delete,
            })),
        },
        Handle {
            site: 1,
            target: 1,
            container: 0,
            action: Action(ActionInner::Tree(TreeAction {
                target: (1, 7),
                action: TreeActionInner::Delete,
            })),
        },
        Checkout { site: 2, to: 5 },
    ])
}

#[test]
fn same_peer_have_same_position() {
    test_actions(vec![
        Handle {
            site: 3,
            target: 3,
            container: 3,
            action: Generic(GenericAction {
                value: I32(51577603),
                bool: true,
                key: 50725635,
                pos: 217020518514230019,
                length: 217020518514230019,
                prop: 217020905061286659,
            }),
        },
        Handle {
            site: 3,
            target: 3,
            container: 3,
            action: Generic(GenericAction {
                value: I32(-1325202685),
                bool: true,
                key: 3719410097,
                pos: 15974634357927293361,
                length: 217020518514230019,
                prop: 217020518514230019,
            }),
        },
        Sync { from: 177, to: 177 },
        Sync { from: 1, to: 177 },
        Handle {
            site: 9,
            target: 31,
            container: 31,
            action: Generic(GenericAction {
                value: I32(-585162977),
                bool: true,
                key: 2981167537,
                pos: 12800922302317527985,
                length: 217020518514230193,
                prop: 3832616292335158019,
            }),
        },
        Handle {
            site: 3,
            target: 3,
            container: 3,
            action: Generic(GenericAction {
                value: I32(50529027),
                bool: true,
                key: 50529027,
                pos: 5379,
                length: 0,
                prop: 217020518514230016,
            }),
        },
        Handle {
            site: 3,
            target: 93,
            container: 3,
            action: Generic(GenericAction {
                value: I32(50529027),
                bool: true,
                key: 50529027,
                pos: 12804210589408887555,
                length: 5570865884914900401,
                prop: 217020518514286001,
            }),
        },
        Sync { from: 177, to: 177 },
        Sync { from: 3, to: 3 },
        Handle {
            site: 3,
            target: 3,
            container: 3,
            action: Generic(GenericAction {
                value: I32(50529027),
                bool: true,
                key: 50529027,
                pos: 18446742987133354755,
                length: 12804018527061345279,
                prop: 12754670997176693169,
            }),
        },
        Handle {
            site: 3,
            target: 3,
            container: 3,
            action: Generic(GenericAction {
                value: I32(50529027),
                bool: true,
                key: 50529027,
                pos: 217212583781008133,
                length: 12804210592328123139,
                prop: 217020518514274726,
            }),
        },
        Sync { from: 177, to: 177 },
        Sync { from: 33, to: 34 },
        Handle {
            site: 31,
            target: 31,
            container: 31,
            action: Generic(GenericAction {
                value: I32(2032387),
                bool: true,
                key: 522133279,
                pos: 18446744069936717599,
                length: 2422265009078796287,
                prop: 2242545357980376866,
            }),
        },
        Handle {
            site: 3,
            target: 3,
            container: 3,
            action: Generic(GenericAction {
                value: I32(-1322188359),
                bool: true,
                key: 3719374257,
                pos: 553890225,
                length: 0,
                prop: 0,
            }),
        },
    ])
}

#[test]
fn btree() {
    test_actions(vec![
        Handle {
            site: 47,
            target: 47,
            container: 47,
            action: Generic(GenericAction {
                value: Container(Text),
                bool: true,
                key: 603255842,
                pos: 4405365956304642851,
                length: 2531906049332683555,
                prop: 2531906049332683555,
            }),
        },
        Handle {
            site: 35,
            target: 35,
            container: 35,
            action: Generic(GenericAction {
                value: I32(-199023837),
                bool: false,
                key: 4109694196,
                pos: 3399988124715003695,
                length: 11745387828175253295,
                prop: 10806250015152668671,
            }),
        },
        Handle {
            site: 35,
            target: 35,
            container: 35,
            action: Generic(GenericAction {
                value: I32(589505315),
                bool: true,
                key: 589505315,
                pos: 2531906049332683555,
                length: 18443635706894164958,
                prop: 2531906049334387456,
            }),
        },
        Handle {
            site: 35,
            target: 35,
            container: 35,
            action: Generic(GenericAction {
                value: I32(589505315),
                bool: true,
                key: 589505315,
                pos: 17651001271322354467,
                length: 15842537541752648948,
                prop: 13112093928211176340,
            }),
        },
        Checkout {
            site: 149,
            to: 4152595931,
        },
        Handle {
            site: 35,
            target: 35,
            container: 35,
            action: Generic(GenericAction {
                value: I32(589505315),
                bool: true,
                key: 589505315,
                pos: 3455656147018904611,
                length: 3399988123394780975,
                prop: 18420566900875433263,
            }),
        },
        Sync { from: 247, to: 149 },
        Handle {
            site: 35,
            target: 35,
            container: 35,
            action: Generic(GenericAction {
                value: I32(589505315),
                bool: true,
                key: 589505315,
                pos: 2531906049332683555,
                length: 18443635706894164771,
                prop: 2531906049334387456,
            }),
        },
        Handle {
            site: 35,
            target: 35,
            container: 35,
            action: Generic(GenericAction {
                value: I32(589505315),
                bool: true,
                key: 589505315,
                pos: 17651001271322354467,
                length: 3399988123394780975,
                prop: 18420566900875433263,
            }),
        },
        Sync { from: 247, to: 149 },
        Handle {
            site: 35,
            target: 35,
            container: 35,
            action: Generic(GenericAction {
                value: I32(589505315),
                bool: true,
                key: 589505315,
                pos: 2531906049332683555,
                length: 18443635706894164771,
                prop: 2531906049334387456,
            }),
        },
        Handle {
            site: 35,
            target: 35,
            container: 35,
            action: Generic(GenericAction {
                value: I32(589505315),
                bool: true,
                key: 589505315,
                pos: 17651001271322354467,
                length: 15842537541752648948,
                prop: 13112093928211176340,
            }),
        },
        Checkout {
            site: 149,
            to: 4152595931,
        },
        Handle {
            site: 35,
            target: 35,
            container: 35,
            action: Generic(GenericAction {
                value: I32(589505315),
                bool: true,
                key: 589505315,
                pos: 3455656147018904611,
                length: 3399988123394780975,
                prop: 18420566900875433263,
            }),
        },
        Sync { from: 247, to: 149 },
        Handle {
            site: 35,
            target: 35,
            container: 35,
            action: Generic(GenericAction {
                value: I32(589505315),
                bool: true,
                key: 589505315,
                pos: 2531906049332683555,
                length: 18443635706894164771,
                prop: 2531906049334387456,
            }),
        },
        Handle {
            site: 35,
            target: 35,
            container: 35,
            action: Generic(GenericAction {
                value: I32(589505315),
                bool: true,
                key: 589505315,
                pos: 17651001271322354467,
                length: 15842537541752648948,
                prop: 13112093928211176340,
            }),
        },
        Checkout {
            site: 149,
            to: 4152595931,
        },
        Handle {
            site: 47,
            target: 47,
            container: 47,
            action: Generic(GenericAction {
                value: I32(2117021487),
                bool: true,
                key: 4109643567,
                pos: 10762350610448643316,
                length: 15842537271212635380,
                prop: 3456375918687509396,
            }),
        },
        Handle {
            site: 47,
            target: 47,
            container: 47,
            action: Generic(GenericAction {
                value: I32(796798767),
                bool: true,
                key: 791621423,
                pos: 18416867685734612885,
                length: 4048507366587489243,
                prop: 3328119647583022848,
            }),
        },
        Handle {
            site: 47,
            target: 47,
            container: 244,
            action: Generic(GenericAction {
                value: Container(List),
                bool: true,
                key: 2213868693,
                pos: 10762359406541649399,
                length: 4048506473091929563,
                prop: 3328119645351653120,
            }),
        },
        Handle {
            site: 47,
            target: 47,
            container: 47,
            action: Generic(GenericAction {
                value: Container(Tree),
                bool: true,
                key: 4109694207,
                pos: 10762359406541665524,
                length: 15842537271212635380,
                prop: 3456375918687509396,
            }),
        },
        Handle {
            site: 47,
            target: 47,
            container: 47,
            action: Generic(GenericAction {
                value: I32(796798767),
                bool: true,
                key: 4109693999,
                pos: 15842256114205982708,
                length: 18446744073518013324,
                prop: 18446744073709551615,
            }),
        },
        Handle {
            site: 213,
            target: 149,
            container: 149,
            action: Generic(GenericAction {
                value: I32(771),
                bool: false,
                key: 0,
                pos: 6326486836992,
                length: 217020518514230019,
                prop: 217020518514230019,
            }),
        },
        Handle {
            site: 3,
            target: 3,
            container: 3,
            action: Generic(GenericAction {
                value: I32(50529027),
                bool: true,
                key: 197379,
                pos: 217020518514230144,
                length: 2233785415175766015,
                prop: 217020518463702465,
            }),
        },
        Handle {
            site: 3,
            target: 3,
            container: 3,
            action: Generic(GenericAction {
                value: I32(1283),
                bool: false,
                key: 520093696,
                pos: 0,
                length: 72905330438374147,
                prop: 18446742987133354755,
            }),
        },
        Handle {
            site: 35,
            target: 35,
            container: 47,
            action: Generic(GenericAction {
                value: Container(Text),
                bool: true,
                key: 603255842,
                pos: 4405365956304642851,
                length: 2531906049332683555,
                prop: 2531906049332683555,
            }),
        },
        Handle {
            site: 35,
            target: 35,
            container: 35,
            action: Generic(GenericAction {
                value: I32(-199023837),
                bool: false,
                key: 4109694196,
                pos: 3399988124715003695,
                length: 11745387828175253295,
                prop: 10806250015152668671,
            }),
        },
        Handle {
            site: 35,
            target: 35,
            container: 35,
            action: Generic(GenericAction {
                value: I32(589505315),
                bool: true,
                key: 589505315,
                pos: 2531906049332683555,
                length: 18443635706894164771,
                prop: 2531906049334387456,
            }),
        },
        Handle {
            site: 35,
            target: 35,
            container: 35,
            action: Generic(GenericAction {
                value: I32(589505315),
                bool: true,
                key: 589505315,
                pos: 17651001271322354467,
                length: 15842537541752648948,
                prop: 13112093928211176340,
            }),
        },
        Checkout {
            site: 149,
            to: 4152595931,
        },
        Handle {
            site: 35,
            target: 35,
            container: 35,
            action: Generic(GenericAction {
                value: I32(589520163),
                bool: true,
                key: 589505315,
                pos: 3455656147018904611,
                length: 3399988123394780975,
                prop: 18420566900875433263,
            }),
        },
        Sync { from: 247, to: 149 },
        Handle {
            site: 35,
            target: 35,
            container: 35,
            action: Generic(GenericAction {
                value: I32(589505315),
                bool: true,
                key: 589505315,
                pos: 2531906049332683555,
                length: 18443635706894164771,
                prop: 2531906049334387456,
            }),
        },
        Handle {
            site: 35,
            target: 35,
            container: 35,
            action: Generic(GenericAction {
                value: I32(589505315),
                bool: true,
                key: 589505315,
                pos: 268431750538019,
                length: 0,
                prop: 0,
            }),
        },
    ])
}

#[test]
fn tree_compose() {
    test_actions(vec![
        Handle {
            site: 0,
            target: 0,
            container: 0,
            action: Generic(GenericAction {
                value: I32(67108864),
                bool: false,
                key: 5120,
                pos: 18374967950957286400,
                length: 2244797026329624582,
                prop: 18434758041542467359,
            }),
        },
        SyncAll,
        Handle {
            site: 4,
            target: 0,
            container: 0,
            action: Generic(GenericAction {
                value: I32(0),
                bool: false,
                key: 0,
                pos: 0,
                length: 0,
                prop: 18446524175678963712,
            }),
        },
        Checkout {
            site: 0,
            to: 520093727,
        },
        Handle {
            site: 126,
            target: 0,
            container: 0,
            action: Generic(GenericAction {
                value: Container(Counter),
                bool: true,
                key: 3520188881,
                pos: 6872316421537386961,
                length: 6872316419617283935,
                prop: 6872316419617283935,
            }),
        },
        Undo {
            site: 95,
            op_len: 1600085855,
        },
        Undo {
            site: 95,
            op_len: 1600085855,
        },
        Handle {
            site: 0,
            target: 0,
            container: 0,
            action: Generic(GenericAction {
                value: I32(262144),
                bool: false,
                key: 20,
                pos: 504122782800412436,
                length: 2242554153559866112,
                prop: 9511555592568334879,
            }),
        },
        SyncAll,
        Handle {
            site: 0,
            target: 0,
            container: 0,
            action: Generic(GenericAction {
                value: I32(0),
                bool: false,
                key: 16777216,
                pos: 0,
                length: 4107282860161892352,
                prop: 18390450177879048246,
            }),
        },
        Handle {
            site: 49,
            target: 0,
            container: 31,
            action: Generic(GenericAction {
                value: I32(-519962624),
                bool: true,
                key: 4294967295,
                pos: 15119096123158032686,
                length: 4195730024608485841,
                prop: 4195730024608447034,
            }),
        },
    ]);
}
