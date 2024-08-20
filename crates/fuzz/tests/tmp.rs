use std::sync::Arc;

use fuzz::{
    actions::{
        ActionInner,
        ActionWrapper::{self, *},
        GenericAction,
    },
    container::{MapAction, TextAction, TextActionInner, TreeAction, TreeActionInner},
    crdt_fuzzer::{minify_error, test_multi_sites, Action::*, FuzzTarget, FuzzValue::*},
};
use loro::{ContainerType::*, LoroCounter, LoroDoc};

// #[ctor::ctor]
// fn init() {
//     dev_utils::setup_test_log();
// }

#[test]
fn empty() {
    test_multi_sites(5, vec![FuzzTarget::All], &mut []);
}

#[test]
fn one_op() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [Handle {
            site: 33,
            target: 158,
            container: 0,
            action: Generic(GenericAction {
                value: I32(0),
                bool: false,
                key: 0,
                pos: 0,
                length: 0,
                prop: 0,
            }),
        }],
    );
}

#[test]
fn two_ops() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            SyncAll,
            Handle {
                site: 47,
                target: 190,
                container: 190,
                action: Generic(GenericAction {
                    value: Container(Unknown(0)),
                    bool: false,
                    key: 40,
                    pos: 0,
                    length: 0,
                    prop: 0,
                }),
            },
        ],
    );
}

#[test]
fn next_back() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 200,
                target: 19,
                container: 19,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 320017171,
                    pos: 1374463283923456787,
                    length: 1374472080016478995,
                    prop: 1374463283923456787,
                }),
            },
            Handle {
                site: 19,
                target: 19,
                container: 19,
                action: Generic(GenericAction {
                    value: I32(320017171),
                    bool: true,
                    key: 320017171,
                    pos: 1374463309693260563,
                    length: 1374463283923456787,
                    prop: 57140735609213715,
                }),
            },
            Sync { from: 171, to: 139 },
            Handle {
                site: 171,
                target: 171,
                container: 39,
                action: Generic(GenericAction {
                    value: Container(Counter),
                    bool: false,
                    key: 320017323,
                    pos: 18446743056122319635,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 255,
                target: 19,
                container: 19,
                action: Generic(GenericAction {
                    value: I32(-1),
                    bool: true,
                    key: 2206466047,
                    pos: 9476562641788044163,
                    length: 9476562641788044163,
                    prop: 9476562641788044163,
                }),
            },
            SyncAllUndo {
                site: 131,
                op_len: 2206434179,
            },
            SyncAllUndo {
                site: 131,
                op_len: 2206434179,
            },
            SyncAllUndo {
                site: 131,
                op_len: 2206434179,
            },
            SyncAllUndo {
                site: 19,
                op_len: 320017171,
            },
            Checkout {
                site: 48,
                to: 320017171,
            },
            Handle {
                site: 19,
                target: 19,
                container: 19,
                action: Generic(GenericAction {
                    value: I32(320017171),
                    bool: true,
                    key: 320017171,
                    pos: 1374463283923456787,
                    length: 1374463283923456787,
                    prop: 9476562641788044051,
                }),
            },
            SyncAllUndo {
                site: 131,
                op_len: 2206434179,
            },
            SyncAllUndo {
                site: 131,
                op_len: 327385987,
            },
            Handle {
                site: 19,
                target: 50,
                container: 51,
                action: Generic(GenericAction {
                    value: I32(320017171),
                    bool: true,
                    key: 320017171,
                    pos: 1374463283923456787,
                    length: 1374463283923456787,
                    prop: 1374463283923456787,
                }),
            },
            Handle {
                site: 19,
                target: 19,
                container: 25,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 2206431619,
                    pos: 9476562641788044163,
                    length: 9476562641788044155,
                    prop: 9476562643876577279,
                }),
            },
            SyncAllUndo {
                site: 131,
                op_len: 320056195,
            },
            Handle {
                site: 19,
                target: 19,
                container: 65,
                action: Generic(GenericAction {
                    value: I32(320017171),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 1374463285248917503,
                    prop: 1374452207202800403,
                }),
            },
            Handle {
                site: 19,
                target: 19,
                container: 19,
                action: Generic(GenericAction {
                    value: I32(-1),
                    bool: true,
                    key: 4294967293,
                    pos: 473811445759410175,
                    length: 1873384193460404139,
                    prop: 9476562641788015379,
                }),
            },
            SyncAllUndo {
                site: 131,
                op_len: 2206434179,
            },
            SyncAllUndo {
                site: 131,
                op_len: 2206434179,
            },
            SyncAll,
            SyncAllUndo {
                site: 131,
                op_len: 2206434179,
            },
            Handle {
                site: 19,
                target: 19,
                container: 19,
                action: Generic(GenericAction {
                    value: I32(320017171),
                    bool: true,
                    key: 320017171,
                    pos: 1374463283923456787,
                    length: 17072322735077135123,
                    prop: 1374463283923514604,
                }),
            },
            Sync { from: 0, to: 171 },
            Sync { from: 139, to: 171 },
            Sync { from: 171, to: 39 },
            Sync { from: 50, to: 1 },
            Handle {
                site: 19,
                target: 19,
                container: 19,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 65,
                target: 19,
                container: 19,
                action: Generic(GenericAction {
                    value: I32(320017171),
                    bool: true,
                    key: 4294967295,
                    pos: 9476562641788076031,
                    length: 9476562641788044163,
                    prop: 9476562641788044163,
                }),
            },
            SyncAllUndo {
                site: 131,
                op_len: 2206434179,
            },
            SyncAllUndo {
                site: 131,
                op_len: 2206434179,
            },
            SyncAllUndo {
                site: 131,
                op_len: 2206434179,
            },
            SyncAllUndo {
                site: 131,
                op_len: 327385987,
            },
            Handle {
                site: 19,
                target: 50,
                container: 51,
                action: Generic(GenericAction {
                    value: I32(320017171),
                    bool: true,
                    key: 320017171,
                    pos: 1374463283923456787,
                    length: 1374495169760662291,
                    prop: 1374463283923456787,
                }),
            },
            Handle {
                site: 19,
                target: 19,
                container: 19,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 2206434179,
                    pos: 9476562641788044163,
                    length: 1374463766846210947,
                    prop: 3476835202491421459,
                }),
            },
            Handle {
                site: 19,
                target: 27,
                container: 19,
                action: Generic(GenericAction {
                    value: I32(320017171),
                    bool: true,
                    key: 589505299,
                    pos: 6148859500064613155,
                    length: 6148914691236517205,
                    prop: 6148914691236517205,
                }),
            },
            Checkout {
                site: 85,
                to: 1426064213,
            },
            Checkout {
                site: 85,
                to: 589505365,
            },
            Handle {
                site: 35,
                target: 35,
                container: 35,
                action: Generic(GenericAction {
                    value: I32(589505315),
                    bool: true,
                    key: 589505315,
                    pos: 6148914691233227555,
                    length: 6148914691236517205,
                    prop: 2531961240504587605,
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
                    pos: 6148971865837871907,
                    length: 2531906264923198805,
                    prop: 2531906049332683555,
                }),
            },
            Handle {
                site: 35,
                target: 35,
                container: 35,
                action: Generic(GenericAction {
                    value: I32(-56541),
                    bool: true,
                    key: 4294967295,
                    pos: 2531906997930950655,
                    length: 2531906049332683555,
                    prop: 2531906049332683555,
                }),
            },
            Handle {
                site: 35,
                target: 35,
                container: 35,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 320017171,
                    pos: 1374513861458334483,
                    length: 18380055476874449683,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Handle {
                site: 19,
                target: 19,
                container: 19,
                action: Generic(GenericAction {
                    value: I32(320017171),
                    bool: true,
                    key: 320017352,
                    pos: 18446743056122319635,
                    length: 18446744073709420543,
                    prop: 18422825930478052096,
                }),
            },
            SyncAll,
            SyncAll,
            Sync { from: 255, to: 0 },
            Handle {
                site: 19,
                target: 19,
                container: 19,
                action: Generic(GenericAction {
                    value: I32(320017171),
                    bool: true,
                    key: 320017171,
                    pos: 1374463283923456793,
                    length: 4625217789954822931,
                    prop: 12370169552444260658,
                }),
            },
            SyncAllUndo {
                site: 131,
                op_len: 320033667,
            },
            Handle {
                site: 19,
                target: 19,
                container: 19,
                action: Generic(GenericAction {
                    value: I32(320077823),
                    bool: true,
                    key: 2214531859,
                    pos: 9476562641788044163,
                    length: 12370168821397402243,
                    prop: 9476562641788044203,
                }),
            },
            Handle {
                site: 65,
                target: 19,
                container: 19,
                action: Generic(GenericAction {
                    value: I32(-15527149),
                    bool: true,
                    key: 320017171,
                    pos: 9476562641788075795,
                    length: 9476562641788044163,
                    prop: 9476562641788044163,
                }),
            },
            SyncAllUndo {
                site: 131,
                op_len: 2880134019,
            },
            Handle {
                site: 0,
                target: 47,
                container: 3,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294967295,
                    pos: 1374463283923477011,
                    length: 1374463283754439443,
                    prop: 1374463283923503275,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085791,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600069471,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 129,
                op_len: 1600085887,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 4294967295,
            },
            SyncAll,
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            SyncAllUndo {
                site: 151,
                op_len: 2543294359,
            },
            Sync { from: 191, to: 191 },
            Handle {
                site: 5,
                target: 5,
                container: 5,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 84213760,
                    pos: 17871696215406871813,
                    length: 6872316419617283935,
                    prop: 6872316419617283935,
                }),
            },
            SyncAll,
            Sync { from: 255, to: 9 },
            SyncAll,
            SyncAll,
            SyncAll,
        ],
    );
}
