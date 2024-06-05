use fuzz::{
    actions::{ActionWrapper::*, GenericAction},
    crdt_fuzzer::{test_multi_sites, Action::*, FuzzTarget, FuzzValue::*},
};
use loro_common::ContainerType::*;

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

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

#[test]
fn redo_tree_id_diff() {
    test_multi_sites(
        2,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 51,
                target: 60,
                container: 197,
                action: Generic(GenericAction {
                    value: I32(-296905323),
                    bool: false,
                    key: 2395151462,
                    pos: 6335698875578771752,
                    length: 1716855125946684615,
                    prop: 2807457672376879961,
                }),
            },
            Handle {
                site: 162,
                target: 167,
                container: 90,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 929442508,
                    pos: 4887648083275096983,
                    length: 8237173174339417107,
                    prop: 1571041097810100079,
                }),
            },
            Checkout {
                site: 56,
                to: 1826343396,
            },
            SyncAllUndo {
                site: 10,
                op_len: 998370061,
            },
            Handle {
                site: 112,
                target: 78,
                container: 159,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: false,
                    key: 1978700208,
                    pos: 15377364763518525973,
                    length: 13205966979381542996,
                    prop: 5155832222345785212,
                }),
            },
        ],
    );
}

#[test]
fn tree_delete() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 33,
                target: 147,
                container: 2,
                action: Generic(GenericAction {
                    value: I32(2071690107),
                    bool: true,
                    key: 2223278715,
                    pos: 11357407135578037636,
                    length: 11357407135578037661,
                    prop: 11357407135578037661,
                }),
            },
            SyncAllUndo {
                site: 223,
                op_len: 33721747,
            },
            Handle {
                site: 2,
                target: 2,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(-1971618949),
                    bool: false,
                    key: 2644345988,
                    pos: 11357407135578037661,
                    length: 11357407135578037661,
                    prop: 11357407135578037661,
                }),
            },
            SyncAllUndo {
                site: 157,
                op_len: 2644352413,
            },
        ],
    )
}

#[test]
fn tree_undo_delete_with_diff_old_index() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(454761243),
                    bool: true,
                    key: 4280621851,
                    pos: 1953184669377757183,
                    length: 1953184666628070171,
                    prop: 71829045943205915,
                }),
            },
            Handle {
                site: 251,
                target: 197,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(454761243),
                    bool: true,
                    key: 454761243,
                    pos: 1953184666628070171,
                    length: 16710579922159737627,
                    prop: 288230380914862055,
                }),
            },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(387661595),
                    bool: false,
                    key: 454761243,
                    pos: 1953184666628070171,
                    length: 71829045943205915,
                    prop: 18430413027502194837,
                }),
            },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(454761243),
                    bool: true,
                    key: 454761243,
                    pos: 16710579922159737627,
                    length: 288230380914862055,
                    prop: 1953184666628070171,
                }),
            },
            Handle {
                site: 63,
                target: 27,
                container: 23,
                action: Generic(GenericAction {
                    value: I32(454761243),
                    bool: true,
                    key: 454761243,
                    pos: 1953184666628070171,
                    length: 1953184666628070171,
                    prop: 1953184666627808027,
                }),
            },
            SyncAll,
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(454761243),
                    bool: false,
                    key: 807600128,
                    pos: 29802787832063,
                    length: 163831513883392,
                    prop: 2527082340907941888,
                }),
            },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(-1920103141),
                    bool: true,
                    key: 2374864269,
                    pos: 10199964370168810893,
                    length: 10199964370168810893,
                    prop: 10199964370168810893,
                }),
            },
            SyncAllUndo {
                site: 141,
                op_len: 2374864269,
            },
        ],
    )
}
