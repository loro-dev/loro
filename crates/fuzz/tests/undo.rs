use fuzz::{
    actions::{ActionWrapper::*, GenericAction},
    crdt_fuzzer::{minify_simple, test_multi_sites, Action::*, FuzzTarget, FuzzValue::*},
};
use loro::ContainerType::*;

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

#[test]
fn tree_undo_delete_parent_in_b() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 129,
                target: 207,
                container: 96,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 1478566177,
                    pos: 2387225703656530209,
                    length: 388195770586702113,
                    prop: 18446743116485501224,
                }),
            },
            SyncAll,
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 286331665,
                    pos: 17216961135462248175,
                    length: 1229782938247303441,
                    prop: 1229782938247303441,
                }),
            },
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 286331153,
                    pos: 1229782938247303441,
                    length: 1229782938247303441,
                    prop: 1229782938247303441,
                }),
            },
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331137),
                    bool: true,
                    key: 286331153,
                    pos: 4256201887840276755,
                    length: 1229782946837238033,
                    prop: 1229782938247303441,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 2,
                container: 5,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 2145059327,
                    pos: 4050480110299788081,
                    length: 18157383382424616754,
                    prop: 18157383382357244923,
                }),
            },
            Undo {
                site: 255,
                op_len: 4227596287,
            },
            Handle {
                site: 223,
                target: 47,
                container: 184,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4227595259,
                    pos: 18157383382357244923,
                    length: 2387225703656586235,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Undo {
                site: 17,
                op_len: 3823363055,
            },
            SyncAll,
            Handle {
                site: 17,
                target: 17,
                container: 243,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 286331153,
                    pos: 1229782942240280849,
                    length: 1229782869527826705,
                    prop: 1229785137270558993,
                }),
            },
            Checkout {
                site: 17,
                to: 319885585,
            },
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 286331153,
                    pos: 16501207799683944947,
                    length: 2676586395008832811,
                    prop: 40841467208997,
                }),
            },
            Handle {
                site: 243,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286332177),
                    bool: true,
                    key: 286327027,
                    pos: 1229782938247303441,
                    length: 1229782938247303441,
                    prop: 1229782938247303658,
                }),
            },
            SyncAllUndo {
                site: 135,
                op_len: 2273806215,
            },
        ],
    )
}

#[test]
fn tree_undo_move_parent_deleted_in_b() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 129,
                target: 207,
                container: 96,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 1478566177,
                    pos: 2387225703656530209,
                    length: 388195770586702113,
                    prop: 18446743116485501224,
                }),
            },
            SyncAll,
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 286331665,
                    pos: 17216961135462248175,
                    length: 1229782938247303441,
                    prop: 1229782938247303441,
                }),
            },
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 286331153,
                    pos: 1229782938247303441,
                    length: 1229782938247303441,
                    prop: 1229782938247303441,
                }),
            },
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331137),
                    bool: true,
                    key: 286331153,
                    pos: 4256201887840276755,
                    length: 1229782946837238033,
                    prop: 1229782938247303441,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 2,
                container: 5,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 2145059327,
                    pos: 4050480110299788081,
                    length: 18157383382424616754,
                    prop: 18157383382357244923,
                }),
            },
            // create
            Handle {
                site: 0,
                target: 2,
                container: 5,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 2145059327,
                    pos: 4050480110299788081,
                    length: 18157383382424616754,
                    prop: 18157383382357244923,
                }),
            },
            Handle {
                site: 0,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286332177),
                    bool: true,
                    key: 286327027,
                    pos: 1229782938247303441,
                    length: 1229782938247303441,
                    prop: 1229782938247303658,
                }),
            },
            Handle {
                site: 223,
                target: 47,
                container: 184,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4227595259,
                    pos: 18157383382357244923,
                    length: 2387225703656586235,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Undo {
                site: 17,
                op_len: 3823363055,
            },
            SyncAll,
            Handle {
                site: 17,
                target: 17,
                container: 243,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 286331153,
                    pos: 1229782942240280849,
                    length: 1229782869527826705,
                    prop: 1229785137270558993,
                }),
            },
            Checkout {
                site: 17,
                to: 319885585,
            },
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 286331153,
                    pos: 16501207799683944947,
                    length: 2676586395008832811,
                    prop: 40841467208997,
                }),
            },
            Handle {
                site: 243,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286332177),
                    bool: true,
                    key: 286327027,
                    pos: 1229782938247303441,
                    length: 1229782938247303441,
                    prop: 1229782938247303658,
                }),
            },
            SyncAllUndo {
                site: 135,
                op_len: 2273806215,
            },
        ],
    )
}

#[test]
fn tree_undo_move_deleted_in_b() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 129,
                target: 207,
                container: 96,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 1478566177,
                    pos: 2387225703656530209,
                    length: 388195770586702113,
                    prop: 18446743116485501224,
                }),
            },
            SyncAll,
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 286331665,
                    pos: 17216961135462248175,
                    length: 1229782938247303441,
                    prop: 1229782938247303441,
                }),
            },
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 286331153,
                    pos: 1229782938247303441,
                    length: 1229782938247303441,
                    prop: 1229782938247303441,
                }),
            },
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331137),
                    bool: true,
                    key: 286331153,
                    pos: 4256201887840276755,
                    length: 1229782946837238033,
                    prop: 1229782938247303441,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 2,
                container: 5,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 2145059327,
                    pos: 4050480110299788081,
                    length: 18157383382424616754,
                    prop: 18157383382357244923,
                }),
            },
            // create
            Handle {
                site: 0,
                target: 2,
                container: 5,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 2145059327,
                    pos: 4050480110299788081,
                    length: 18157383382424616754,
                    prop: 18157383382357244923,
                }),
            },
            Handle {
                site: 0,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286332177),
                    bool: true,
                    key: 286327027,
                    pos: 0,
                    length: 1,
                    prop: 2,
                }),
            },
            Handle {
                site: 223,
                target: 47,
                container: 184,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4227595259,
                    pos: 18157383382357244923,
                    length: 2387225703656586235,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Undo {
                site: 17,
                op_len: 3823363055,
            },
            SyncAll,
            Handle {
                site: 17,
                target: 17,
                container: 243,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 286331153,
                    pos: 0,
                    length: 1229782869527826705,
                    prop: 1229785137270558993,
                }),
            },
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 286331153,
                    pos: 0,
                    length: 2676586395008832811,
                    prop: 1,
                }),
            },
            Handle {
                site: 243,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286332177),
                    bool: true,
                    key: 286327027,
                    pos: 1229782938247303441,
                    length: 1229782938247303441,
                    prop: 1229782938247303658,
                }),
            },
            SyncAllUndo {
                site: 135,
                op_len: 2273806215,
            },
        ],
    )
}

#[test]
fn tree_diff_position() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Tree],
        &mut [
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 151650303,
                    pos: 18446744073709488393,
                    length: 18446744073709551607,
                    prop: 2242546323825885183,
                }),
            },
            Handle {
                site: 31,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 4294904073,
                    pos: 18446744039349813247,
                    length: 18446744073709551615,
                    prop: 18446744073709540631,
                }),
            },
            Handle {
                site: 31,
                target: 31,
                container: 120,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 151587327,
                    pos: 17870283321406127881,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Handle {
                site: 31,
                target: 31,
                container: 120,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294904319,
                    pos: 18446744073709551615,
                    length: 2267596630907625247,
                    prop: 18446744073709551391,
                }),
            },
            Handle {
                site: 95,
                target: 120,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294967295,
                    pos: 2267596630907625247,
                    length: 18446744073709551391,
                    prop: 18446472533143846911,
                }),
            },
            Handle {
                site: 31,
                target: 120,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 151587081,
                    pos: 18444492273895866367,
                    length: 18446744073709551615,
                    prop: 18446744072989704191,
                }),
            },
            SyncAllUndo {
                site: 131,
                op_len: 2,
            },
        ],
    )
}

#[test]
fn tree_undo_unknown() {
    // 0: create 13@0 create 0@0 -> 13@0
    // 1: meta 0@0  delete 13@0
    test_multi_sites(
        5,
        vec![FuzzTarget::Tree],
        &mut [
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4281330307,
                    pos: 3423861436305875967,
                    length: 18446744073694871551,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(2015305503),
                    bool: true,
                    key: 4294967071,
                    pos: 18446743798831644671,
                    length: 18446744039349813247,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 3040456650767990783,
                    prop: 18446744073709551607,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 133,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 2039775,
                    pos: 18446744071620984832,
                    length: 9476418040919695327,
                    prop: 18410674826839588863,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(2015305503),
                    bool: true,
                    key: 4294967071,
                    pos: 651333096108457983,
                    length: 1441151880758495497,
                    prop: 18374686479671623680,
                }),
            },
            SyncAll,
            Checkout {
                site: 131,
                to: 536838583,
            },
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4294913857,
                    pos: 18388060938407193507,
                    length: 18446744073709494271,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(522133279),
                    bool: true,
                    key: 4280229752,
                    pos: 18446744073709551615,
                    length: 18446744069566171401,
                    prop: 18446744073709027327,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 31,
                target: 120,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 522133279,
                    pos: 10779248702831402783,
                    length: 9485706711646962581,
                    prop: 18446743254173297663,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(522133279),
                    bool: true,
                    key: 4294967160,
                    pos: 18446744073709551615,
                    length: 18446744073709551615,
                    prop: 2242545357980377087,
                }),
            },
            Handle {
                site: 31,
                target: 120,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Unknown(191)),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709027327,
                    length: 15355022929519706111,
                    prop: 18446744073709551523,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 707911479,
                    pos: 18446744073709551607,
                    length: 9583660007048690651,
                    prop: 18446744073564528789,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Counter),
                    bool: true,
                    key: 4294967295,
                    pos: 2305843009213693951,
                    length: 10778687951896697631,
                    prop: 18386970223563456899,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 553975807,
                    pos: 18446744073560727841,
                    length: 18446744073709551615,
                    prop: 11805368386500689919,
                }),
            },
            Handle {
                site: 31,
                target: 31,
                container: 120,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 522133279,
                    pos: 10922800942115921695,
                    length: 11817444525671159189,
                    prop: 18446743179637817219,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(522133279),
                    bool: true,
                    key: 4280229752,
                    pos: 18428729675200069631,
                    length: 18444492273895866367,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 872415231,
                    pos: 18446744073561321951,
                    length: 71725349863423,
                    prop: 18444310994424758272,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 131,
                container: 131,
                action: Generic(GenericAction {
                    value: I32(-8398026),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 2242545361753210879,
                    prop: 2242545357980376863,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 9,
                target: 9,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: false,
                    key: 4278190080,
                    pos: 18446744073709551607,
                    length: 18420801199931391999,
                    prop: 2267596630907682815,
                }),
            },
            SyncAll,
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4281287043,
                    pos: 3423861436305875967,
                    length: 18446744073694871551,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(522156063),
                    bool: true,
                    key: 4294967295,
                    pos: 651061559686070271,
                    length: 18444492273895866367,
                    prop: 18446744073709551615,
                }),
            },
            Checkout {
                site: 131,
                to: 536838583,
            },
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4294913857,
                    pos: 18446515191345546147,
                    length: 18446744073709494271,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(522133279),
                    bool: true,
                    key: 4294967160,
                    pos: 18446744073709551615,
                    length: 18446744073709551615,
                    prop: 2242545357980377087,
                }),
            },
            Handle {
                site: 31,
                target: 120,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Unknown(191)),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709027327,
                    length: 15355022929519706111,
                    prop: 18446744073709551523,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 707911479,
                    pos: 18446744073709551607,
                    length: 9583660007048690651,
                    prop: 18446744073564528789,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Counter),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 2242792614430507007,
                    prop: 2242545357980376863,
                }),
            },
            Handle {
                site: 31,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 4294904073,
                    pos: 335544319,
                    length: 18446744039333036032,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Handle {
                site: 120,
                target: 31,
                container: 59,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 522133279,
                    pos: 10778687951896697631,
                    length: 18386970223563456899,
                    prop: 18383693675428577237,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(2015305503),
                    bool: true,
                    key: 5407,
                    pos: 2305841909702066176,
                    length: 10736644025422389023,
                    prop: 18446743616657790357,
                }),
            },
            SyncAll,
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(527965983),
                    bool: true,
                    key: 4294967295,
                    pos: 10778763175739260927,
                    length: 18387987836983154581,
                    prop: 2267596630907625247,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-2049),
                    bool: true,
                    key: 4294967295,
                    pos: 18420801199931391999,
                    length: 2267596630907682815,
                    prop: 4,
                }),
            },
            SyncAll,
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4281287043,
                    pos: 3423861436305875967,
                    length: 18446744073694871551,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(522156063),
                    bool: true,
                    key: 33554432,
                    pos: 2242546323809107968,
                    length: 10778685111367573279,
                    prop: 18446744073702577559,
                }),
            },
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(522133279),
                    bool: true,
                    key: 4280229752,
                    pos: 18446744073709551615,
                    length: 9481649068780656091,
                    prop: 15420091632514445121,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(522156063),
                    bool: true,
                    key: 4294967295,
                    pos: 651062616234196991,
                    length: 17870283321406127881,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            // 0@0 meta
            Handle {
                site: 31,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Map),
                    bool: true,
                    key: 4294967167,
                    pos: 18446744073709551615,
                    length: 2305843009213693951,
                    prop: 2242545332210573087,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 939524095,
                    pos: 18446744073561321951,
                    length: 71725349863423,
                    prop: 18444310994424758272,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 555819297,
                    pos: 18446744035610665249,
                    length: 18446744073709551615,
                    prop: 15355022929519706111,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-1785358849),
                    bool: true,
                    key: 4294967259,
                    pos: 18446744035762757428,
                    length: 18361689565036543,
                    prop: 17823875776802455552,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 131,
                action: Generic(GenericAction {
                    value: I32(555819297),
                    bool: true,
                    key: 555819297,
                    pos: 2387225703656530209,
                    length: 2387225703656530209,
                    prop: 2387225703656530209,
                }),
            },
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 151650303,
                    pos: 1441151880758495497,
                    length: 18374686479671623680,
                    prop: 18446744073709551607,
                }),
            },
            SyncAll,
            Handle {
                site: 31,
                target: 0,
                container: 49,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 555819297,
                    pos: 2387225703656530209,
                    length: 2387225703656530209,
                    prop: 2387225703656530209,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 133,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 2039775,
                    pos: 159580160,
                    length: 648518344244199424,
                    prop: 18446744073701153590,
                }),
            },
            SyncAll,
            Undo {
                site: 31,
                op_len: 2,
            },
        ],
    )
}

#[test]
fn undo_tree_index() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Tree],
        &mut [
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(188430649),
                    bool: true,
                    key: 185273099,
                    pos: 18446744070374634251,
                    length: 795741901218843451,
                    prop: 795741901218843403,
                }),
            },
            Handle {
                site: 1,
                target: 0,
                container: 11,
                action: Generic(GenericAction {
                    value: Container(Counter),
                    bool: true,
                    key: 3654932953,
                    pos: 15697817505862638041,
                    length: 4035108562632563161,
                    prop: 3399988123389603733,
                }),
            },
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 188430649,
                    pos: 795741901218843403,
                    length: 795741901218843403,
                    prop: 2970615681721645323,
                }),
            },
            Handle {
                site: 128,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(185280777),
                    bool: true,
                    key: 185273099,
                    pos: 795741901218843403,
                    length: 15697590118234390529,
                    prop: 15697817505862638041,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690563369,
                    pos: 2965947086361143593,
                    length: 2965947086361143593,
                    prop: 2965947086361143593,
                }),
            },
            SyncAllUndo {
                site: 43,
                op_len: 2214581759,
            },
        ],
    )
}

#[test]
fn undo_tree_delete_delete() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Tree],
        &mut [
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4294913857,
                    pos: 18388060938407193507,
                    length: 9952409283403775,
                    prop: 18446744070941246465,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 555819297,
                    pos: 18446744073560727841,
                    length: 18446744073709551615,
                    prop: 2242545357995114495,
                }),
            },
            Handle {
                site: 120,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Counter),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709027327,
                    length: 15355022929519706111,
                    prop: 18446744073709551523,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 707911479,
                    pos: 18446744073709551607,
                    length: 9583660007048690651,
                    prop: 18446744073564528789,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Counter),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 2242792614430507007,
                    prop: 2242545357980376863,
                }),
            },
            Handle {
                site: 0,
                target: 174,
                container: 1,
                action: Generic(GenericAction {
                    value: I32(-65536),
                    bool: true,
                    key: 4294967295,
                    pos: 15355022929519706111,
                    length: 2242545361753210787,
                    prop: 2305704159417671544,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4146737631,
                    pos: 15852670688344145919,
                    length: 10774017683553796411,
                    prop: 18446744073708985120,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 131,
                action: Generic(GenericAction {
                    value: Container(Counter),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 2242792614430507007,
                    prop: 2242545357980376863,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 133,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 2039775,
                    pos: 648518344252784640,
                    length: 18446744073701153590,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(522156063),
                    bool: true,
                    key: 4294967295,
                    pos: 651061559686070271,
                    length: 21990232555519,
                    prop: 18444491174384238592,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-1785341153),
                    bool: true,
                    key: 2207618455,
                    pos: 15420091632514445121,
                    length: 15852424397725860863,
                    prop: 6556963984818527145,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 555819297,
                    pos: 18446744073560727841,
                    length: 18446627525477007359,
                    prop: 18446462667452317695,
                }),
            },
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(527965983),
                    bool: true,
                    key: 4294967295,
                    pos: 651062616248025087,
                    length: 17870283321406127881,
                    prop: 18386970223563456899,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 892679477,
                    pos: 3834029160418063669,
                    length: 3834029160418063669,
                    prop: 3834029160418063669,
                }),
            },
            SyncAllUndo {
                site: 255,
                op_len: 3,
            },
        ],
    )
}

#[test]
fn tree_undo_nested_map_tree_tree_meta() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Tree],
        &mut [
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 872415231,
                    pos: 18446744073561321951,
                    length: 71725349863423,
                    prop: 18444310994424758272,
                }),
            },
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4281287043,
                    pos: 3423861436305875967,
                    length: 18446744073694871551,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4294913857,
                    pos: 18446515191345546147,
                    length: 18446744073709494271,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 31,
                target: 120,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Unknown(191)),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709027327,
                    length: 15355022929519706111,
                    prop: 18446744073709551523,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 707911479,
                    pos: 18446744073709551607,
                    length: 9583660007048690651,
                    prop: 18446744073564528789,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Counter),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 2242792614430507007,
                    prop: 10778762209893752607,
                }),
            },
            Handle {
                site: 1,
                target: 4,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 255,
                    pos: 18446743004262694912,
                    length: 2387225703656530431,
                    prop: 18446744035610665249,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4146737631,
                    pos: 15852670688344145919,
                    length: 10774017683553796411,
                    prop: 18446744073708985120,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 4294967049,
                    pos: 1310719,
                    length: 18446744073575268352,
                    prop: 1729382256910270463,
                }),
            },
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 792822677,
                    pos: 9511556229955321855,
                    length: 18446744069951455023,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Handle {
                site: 31,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4160749567,
                    pos: 18446744073709551615,
                    length: 18446642734358855679,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 939524095,
                    pos: 18446744073561321951,
                    length: 71725349863423,
                    prop: 18444310994424758272,
                }),
            },
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 522133279,
                    pos: 10778686051533659935,
                    length: 18446514557159839127,
                    prop: 18446515191345546147,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-1785358849),
                    bool: true,
                    key: 4294967259,
                    pos: 18446744035762757427,
                    length: 18361689565036543,
                    prop: 17823875776802455552,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 131,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 2242546323825885183,
                    prop: 2242545357980376863,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 4294967049,
                    pos: 1310719,
                    length: 18446744073575268352,
                    prop: 1729382256910270463,
                }),
            },
            Handle {
                site: 31,
                target: 120,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 522133279,
                    pos: 10779248702831402783,
                    length: 9485706711646962581,
                    prop: 2305843005721226239,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 522133279,
                    pos: 2242545357980376863,
                    length: 18446744073694814072,
                    prop: 18446744073705357311,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 131,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 4294967295,
                    pos: 4313322543114092543,
                    length: 2347929015790075969,
                    prop: 18446744073709549403,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 131,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 2242546323825885183,
                    prop: 2242545357980376863,
                }),
            },
            Handle {
                site: 213,
                target: 163,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(527965983),
                    bool: true,
                    key: 4286691203,
                    pos: 2242545357980376863,
                    length: 10779248702831402783,
                    prop: 9485706711646962581,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 131,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 2242546323825885183,
                    prop: 2242545357980376863,
                }),
            },
            SyncAllUndo {
                site: 31,
                op_len: 1,
            },
        ],
    )
}

#[test]
fn tree_undo_delete_and_create_exist_node() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Tree],
        &mut [
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(67108864),
                    bool: false,
                    key: 5120,
                    pos: 18374967954648273920,
                    length: 2244797026329624582,
                    prop: 18434758041542467359,
                }),
            },
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
                    prop: 18446521976655708160,
                }),
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
                    value: I32(47),
                    bool: false,
                    key: 0,
                    pos: 0,
                    length: 4107282860161892352,
                    prop: 18390450177879048246,
                }),
            },
            Handle {
                site: 48,
                target: 0,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(520093696),
                    bool: false,
                    key: 0,
                    pos: 72349003438748113,
                    length: 72340172853149953,
                    prop: 6872316418034041089,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(47),
                    bool: false,
                    key: 0,
                    pos: 0,
                    length: 4107282860161892352,
                    prop: 18390450177879048246,
                }),
            },
            Handle {
                site: 48,
                target: 0,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(-256),
                    bool: true,
                    key: 335544319,
                    pos: 2115960832,
                    length: 72349003438748113,
                    prop: 72340172853149953,
                }),
            },
            Undo {
                site: 95,
                op_len: 1600085855,
            },
            SyncAllUndo {
                site: 128,
                op_len: 4294967249,
            },
            Handle {
                site: 131,
                target: 31,
                container: 39,
                action: Generic(GenericAction {
                    value: I32(-714423189),
                    bool: true,
                    key: 1364283729,
                    pos: 18446744073709551441,
                    length: 14430449448537641246,
                    prop: 15132094744467078979,
                }),
            },
        ],
    )
}

#[test]
fn tree_move_child_whose_parent_deleted() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Tree],
        &mut [
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(67108864),
                    bool: false,
                    key: 5120,
                    pos: 18374967954648273920,
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
                    key: 0,
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
                    value: I32(520093696),
                    bool: false,
                    key: 0,
                    pos: 15119096123158032849,
                    length: 15119095435963257297,
                    prop: 6872316420712079525,
                }),
            },
            SyncAllUndo {
                site: 95,
                op_len: 1600085855,
            },
            SyncAll,
        ],
    )
}

#[test]
fn tree_meta_unknown() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Tree],
        &mut [
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4294913857,
                    pos: 18446515191345546147,
                    length: 18446744073709494271,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 31,
                target: 31,
                container: 120,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 151587327,
                    pos: 1441151086189608713,
                    length: 18374686479671623680,
                    prop: 18446744073709551607,
                }),
            },
            // Handle {
            //     site: 213,
            //     target: 163,
            //     container: 255,
            //     action: Generic(GenericAction {
            //         value: I32(527965983),
            //         bool: true,
            //         key: 4286691203,
            //         pos: 2242545357980376863,
            //         length: 10779248702831402783,
            //         prop: 3144638436309304213,
            //     }),
            // },
            // Handle {
            //     site: 31,
            //     target: 31,
            //     container: 31,
            //     action: Generic(GenericAction {
            //         value: I32(527965983),
            //         bool: true,
            //         key: 4294967295,
            //         pos: 651062616248025087,
            //         length: 17870283321406127881,
            //         prop: 18446744073709551615,
            //     }),
            // },
            Handle {
                site: 31,
                target: 120,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 522133279,
                    pos: 10779248702831402783,
                    length: 9485706711646962581,
                    prop: 18446743254173297663,
                }),
            },
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(522133279),
                    bool: true,
                    key: 4294967160,
                    pos: 18446744073709551615,
                    length: 18446744073709551615,
                    prop: 2242545357980377087,
                }),
            },
            Handle {
                site: 31,
                target: 120,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Unknown(191)),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709027327,
                    length: 15355022929519706111,
                    prop: 18446744073709551523,
                }),
            },
            Handle {
                site: 31,
                target: 219,
                container: 149,
                action: Generic(GenericAction {
                    value: Container(Map),
                    bool: true,
                    key: 4281050111,
                    pos: 18383693675428577237,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Handle {
                site: 120,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Counter),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709027327,
                    length: 15355022929519706111,
                    prop: 18446744073709551523,
                }),
            },
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Unknown(31)),
                    bool: true,
                    key: 3676249887,
                    pos: 4720819787047212437,
                    length: 18446744069448138543,
                    prop: 18387634328600313855,
                }),
            },
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 522133503,
                    pos: 2242545357980415007,
                    length: 18446496818752593695,
                    prop: 720575940379279359,
                }),
            },
            Handle {
                site: 31,
                target: 234,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(522140447),
                    bool: true,
                    key: 522133279,
                    pos: 18446496818752593695,
                    length: 18446744073709551615,
                    prop: 18446743017147596799,
                }),
            },
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4281287043,
                    pos: 18388150188524151807,
                    length: 18446744073694871551,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 792822677,
                    pos: 9511556229955321855,
                    length: 18446744069951455023,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 31,
                target: 31,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(527965983),
                    bool: true,
                    key: 4294967295,
                    pos: 651062616248025087,
                    length: 17870283321406127881,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 31,
                target: 120,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 31,
                    pos: 15795822638653211523,
                    length: 18446744073709551487,
                    prop: 18446744073709551615,
                }),
            },
            Undo {
                site: 31,
                op_len: 7,
            },
        ],
    )
}

#[test]
fn tree_small_issue() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Tree],
        &mut [
            Handle {
                site: 63,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-1761484928),
                    bool: false,
                    key: 513,
                    pos: 2341377969152,
                    length: 18380315979205849600,
                    prop: 4251405740540952575,
                }),
            },
            Handle {
                site: 10,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(40528415),
                    bool: false,
                    key: 3238038528,
                    pos: 0,
                    length: 0,
                    prop: 18446744073692774400,
                }),
            },
            SyncAll,
            Handle {
                site: 33,
                target: 2,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(335577214),
                    bool: true,
                    key: 16777215,
                    pos: 2836986853897275135,
                    length: 7667975533558178817,
                    prop: 10746995183846424578,
                }),
            },
            SyncAllUndo {
                site: 155,
                op_len: 2610666395,
            },
            SyncAllUndo {
                site: 155,
                op_len: 2610666395,
            },
            Checkout {
                site: 155,
                to: 2610666296,
            },
            SyncAll,
        ],
    )
}

#[test]
fn tree_remap() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Tree],
        &mut [
            Handle {
                site: 0,
                target: 1,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4294913857,
                    pos: 18388060938407193507,
                    length: 9952409283403775,
                    prop: 18446744070941246465,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-1785358849),
                    bool: true,
                    key: 4294967259,
                    pos: 18446744035762757431,
                    length: 18361689565036543,
                    prop: 17823875776802455552,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 131,
                action: Generic(GenericAction {
                    value: Container(Counter),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 2242792614430507007,
                    prop: 2242545357980376863,
                }),
            },
            Handle {
                site: 120,
                target: 31,
                container: 59,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 522133279,
                    pos: 10778687951896697631,
                    length: 18386970223563456899,
                    prop: 18383693675428577237,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: false,
                    key: 4146737631,
                    pos: 15852670688344145919,
                    length: 10774017683553796411,
                    prop: 18446744073708985120,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 131,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 2242546323825885183,
                    prop: 2242545357980376863,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 4294967049,
                    pos: 15263775559043514367,
                    length: 15263776468834178003,
                    prop: 15263776468834178003,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 3575119871,
                    pos: 2242545361753210787,
                    length: 2305704159417671544,
                    prop: 2242545357980376863,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 892679477,
                    pos: 3834029160418063669,
                    length: 3834029160418063669,
                    prop: 3834029160418063669,
                }),
            },
            Handle {
                site: 120,
                target: 31,
                container: 59,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 522133279,
                    pos: 10779248702819588895,
                    length: 3144638436309304213,
                    prop: 2305842113780110847,
                }),
            },
            // SyncAll,
            Handle {
                site: 33,
                target: 33,
                container: 33,
                action: Generic(GenericAction {
                    value: I32(555819297),
                    bool: true,
                    key: 555819297,
                    pos: 2387225703656530209,
                    length: 2387225703656530209,
                    prop: 2387225703656530209,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4146737631,
                    pos: 15852670688344145919,
                    length: 10774017683553796411,
                    prop: 18446744073708985120,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 131,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 2242546323825885183,
                    prop: 2242545357846159135,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 4294967049,
                    pos: 1310719,
                    length: 18446744073575268352,
                    prop: 1729382256910270463,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 31,
                action: Generic(GenericAction {
                    value: I32(522133279),
                    bool: true,
                    key: 2015305503,
                    pos: 18446744073709494047,
                    length: 18446744073709551615,
                    prop: 18446744073709549567,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 792822677,
                    pos: 9511556229955321855,
                    length: 18446735273858432815,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 555819297,
                    pos: 18446744073560727841,
                    length: 18446744073709551615,
                    prop: 15355022929519705906,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4146737631,
                    pos: 15852670688344145919,
                    length: 10774017683553796411,
                    prop: 18446744073708985120,
                }),
            },
            SyncAll,
            Handle {
                site: 213,
                target: 6,
                container: 163,
                action: Generic(GenericAction {
                    value: I32(2015305503),
                    bool: true,
                    key: 2176287547,
                    pos: 2242545357980377087,
                    length: 10922800942115921695,
                    prop: 11817444525671159189,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 707911475,
                    pos: 18446744073709551607,
                    length: 9583660007048690651,
                    prop: 18446744073564528789,
                }),
            },
            SyncAllUndo {
                site: 153,
                op_len: 1,
            },
        ],
    )
}

#[test]
fn tree_metadata() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Tree],
        &mut [
            Handle {
                site: 219,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 12395099,
                    pos: 3298534883477,
                    length: 3834868070660322304,
                    prop: 504403158252466996,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 2913840557,
                    pos: 3765062388551930802,
                    length: 12514849900987264429,
                    prop: 12514849905282231725,
                }),
            },
            SyncAll,
            Handle {
                site: 31,
                target: 120,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 151587081,
                    pos: 18444492273895866367,
                    length: 18446744073709551615,
                    prop: 2242545357995114495,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 2242792614424466711,
                    prop: 10749528904694701855,
                }),
            },
            Handle {
                site: 31,
                target: 31,
                container: 120,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294904319,
                    pos: 18446744073709551615,
                    length: 2267596630907625247,
                    prop: 18446744073709551391,
                }),
            },
            SyncAll,
            Handle {
                site: 95,
                target: 120,
                container: 31,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294967295,
                    pos: 3423861436305055519,
                    length: 18410858213187518463,
                    prop: 12214771541103083519,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073169868799,
                    length: 18446496843029282815,
                    prop: 4035224870267125759,
                }),
            },
            SyncAllUndo {
                site: 65,
                op_len: 2751463215,
            },
        ],
    )
}

#[test]
fn tree_metadata2() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 171,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(50529161),
                    bool: true,
                    key: 2769155,
                    pos: 416717214419337,
                    length: 4412750447665283201,
                    prop: 3544668469065809725,
                }),
            },
            SyncAll,
            Handle {
                site: 161,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(454761243),
                    bool: true,
                    key: 4294967067,
                    pos: 3544677320168046591,
                    length: 4412750542920560945,
                    prop: 4268729913046809901,
                }),
            },
            Handle {
                site: 3,
                target: 3,
                container: 3,
                action: Generic(GenericAction {
                    value: I32(707430793),
                    bool: true,
                    key: 4278387587,
                    pos: 1099511627775,
                    length: 9354488261646483456,
                    prop: 13114482111674842904,
                }),
            },
            Handle {
                site: 3,
                target: 7,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(-85),
                    bool: true,
                    key: 59310721,
                    pos: 9871936841907897091,
                    length: 9295431258694322569,
                    prop: 4412750542749796608,
                }),
            },
            SyncAll,
            Handle {
                site: 7,
                target: 7,
                container: 7,
                action: Generic(GenericAction {
                    value: I32(2071690235),
                    bool: true,
                    key: 125533051,
                    pos: 506381209866404351,
                    length: 10055130593152665351,
                    prop: 506381210470516487,
                }),
            },
            Handle {
                site: 7,
                target: 7,
                container: 7,
                action: Generic(GenericAction {
                    value: I32(511),
                    bool: false,
                    key: 16842752,
                    pos: 506381209866536706,
                    length: 8897841259083463547,
                    prop: 18446742995706251131,
                }),
            },
            Handle {
                site: 7,
                target: 7,
                container: 7,
                action: Generic(GenericAction {
                    value: I32(84016903),
                    bool: true,
                    key: 4294967295,
                    pos: 144680349937371135,
                    length: 4702111234470772735,
                    prop: 2821266740684990247,
                }),
            },
            Handle {
                site: 7,
                target: 7,
                container: 7,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 4278680443,
                    pos: 18446470325496907009,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Undo {
                site: 123,
                op_len: 125533051,
            },
        ],
    )
}

#[test]
fn tree_unknown2() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Tree],
        &mut [
            Handle {
                site: 16,
                target: 16,
                container: 16,
                action: Generic(GenericAction {
                    value: I32(1406210064),
                    bool: true,
                    key: 2036949345,
                    pos: 7017023257055951225,
                    length: 18446689516373762401,
                    prop: 14073748835532799,
                }),
            },
            Handle {
                site: 127,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(858993414),
                    bool: true,
                    key: 858989363,
                    pos: 18445899653105791795,
                    length: 7161677112984928256,
                    prop: 7161677110969590627,
                }),
            },
            Handle {
                site: 39,
                target: 39,
                container: 39,
                action: Generic(GenericAction {
                    value: I32(1736337255),
                    bool: true,
                    key: 1734829927,
                    pos: 7451037802321897319,
                    length: 7451037802321897319,
                    prop: 7593457517697918823,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 1734829927,
                    pos: 7451037802321897319,
                    length: 7451037802321897319,
                    prop: 7451037802321897319,
                }),
            },
            Handle {
                site: 253,
                target: 90,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 151584859,
                    pos: 18377384225446139657,
                    length: 18446744073709551615,
                    prop: 651061518279901183,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-1),
                    bool: true,
                    key: 4294967073,
                    pos: 18446744073709551615,
                    length: 12515980216187859455,
                    prop: 12803080277138976173,
                }),
            },
            Handle {
                site: 33,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Unknown(149)),
                    bool: true,
                    key: 906007957,
                    pos: 18446740938383457883,
                    length: 651216092122841087,
                    prop: 10778686051163116296,
                }),
            },
            SyncAll,
            Handle {
                site: 126,
                target: 0,
                container: 57,
                action: Generic(GenericAction {
                    value: Container(Unknown(31)),
                    bool: true,
                    key: 2913840557,
                    pos: 18446744073709530541,
                    length: 10779094167544945663,
                    prop: 18446744069575382498,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-1780744198),
                    bool: true,
                    key: 906007957,
                    pos: 18446740938383457883,
                    length: 12514849901059768319,
                    prop: 12804210592272199085,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: false,
                    key: 8280886,
                    pos: 18446744073709364736,
                    length: 653875205807379938,
                    prop: 10922803139972553481,
                }),
            },
            Handle {
                site: 0,
                target: 255,
                container: 23,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: false,
                    key: 32347,
                    pos: 18446744073709550886,
                    length: 18446499982128185343,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: false,
                    key: 8280886,
                    pos: 18446744073709364736,
                    length: 6201284396160482,
                    prop: 653866370898853888,
                }),
            },
            Handle {
                site: 35,
                target: 35,
                container: 35,
                action: Generic(GenericAction {
                    value: I32(589505315),
                    bool: true,
                    key: 2516450303,
                    pos: 9456393277067466133,
                    length: 18377229688873023266,
                    prop: 18446744073709551615,
                }),
            },
            Undo {
                site: 103,
                op_len: 1734829927,
            },
            Undo {
                site: 0,
                op_len: 151587081,
            },
        ],
    )
}

#[test]
fn tree_parent_remap() {
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
                    key: 454761243,
                    pos: 1953184666628070171,
                    length: 1953184666628070171,
                    prop: 1953184666627808027,
                }),
            },
            Handle {
                site: 65,
                target: 17,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 286326784,
                    pos: 1229782938247303679,
                    length: 1229782938247303441,
                    prop: 1085667750171447569,
                }),
            },
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 21,
                target: 17,
                container: 9,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 286338065,
                    pos: 1229782938247303441,
                    length: 1229782938247303441,
                    prop: 1229782938247303441,
                }),
            },
            Handle {
                site: 21,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 85004561,
                    pos: 1229782938247303434,
                    length: 4398046449985,
                    prop: 1229764173248856064,
                }),
            },
            SyncAll,
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(-286624495),
                    bool: false,
                    key: 4008636142,
                    pos: 1229782938247303441,
                    length: 4369,
                    prop: 1229782938247303424,
                }),
            },
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(355537169),
                    bool: true,
                    key: 286331153,
                    pos: 1229782938247303441,
                    length: 17654171012985520128,
                    prop: 1229782938346782730,
                }),
            },
            SyncAllUndo {
                site: 25,
                op_len: 421112089,
            },
        ],
    )
}

#[test]
fn tree_undo_sort_index() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 187,
                target: 122,
                container: 36,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4287627263,
                    pos: 4902828863,
                    length: 9335720388467884032,
                    prop: 226866784668584321,
                }),
            },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(454761243),
                    bool: true,
                    key: 2812782503,
                    pos: 12080808863958804391,
                    length: 12058485138819360679,
                    prop: 12080808863958804391,
                }),
            },
            SyncAllUndo {
                site: 167,
                op_len: 2812782503,
            },
            SyncAllUndo {
                site: 167,
                op_len: 2812782503,
            },
            SyncAllUndo {
                site: 167,
                op_len: 2812782503,
            },
            SyncAllUndo {
                site: 167,
                op_len: 2812782503,
            },
            SyncAllUndo {
                site: 167,
                op_len: 2812782503,
            },
            SyncAllUndo {
                site: 167,
                op_len: 2812782503,
            },
            SyncAllUndo {
                site: 167,
                op_len: 2812782503,
            },
            SyncAllUndo {
                site: 167,
                op_len: 2812782503,
            },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(454761434),
                    bool: true,
                    key: 454761243,
                    pos: 1953184666628070171,
                    length: 144115188075855871,
                    prop: 4557431447142210354,
                }),
            },
            Handle {
                site: 93,
                target: 52,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(1061109567),
                    bool: true,
                    key: 1061109567,
                    pos: 1953184666628079423,
                    length: 1953184666628070171,
                    prop: 12080808260305368626,
                }),
            },
            SyncAllUndo {
                site: 167,
                op_len: 2812782503,
            },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(454810139),
                    bool: true,
                    key: 454761243,
                    pos: 1953184666628070171,
                    length: 18446744073709551387,
                    prop: 4557573824704098817,
                }),
            },
            SyncAllUndo {
                site: 167,
                op_len: 2812782503,
            },
            SyncAllUndo {
                site: 167,
                op_len: 2812782503,
            },
            SyncAllUndo {
                site: 167,
                op_len: 2812782503,
            },
        ],
    )
}

#[test]
fn minify() {
    minify_simple(
        5,
        |n, actions| test_multi_sites(n, vec![FuzzTarget::Tree], actions),
        |_, actions| actions.to_vec(),
        vec![
	    SyncAll,
	    Handle {
	        site: 31,
	        target: 31,
	        container: 207,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Counter,
	                ),
	                bool: true,
	                key: 3486502863,
	                pos: 14974415777481871311,
	                length: 14974415777481871311,
	                prop: 18446744072901087183,
	            },
	        ),
	    },
	    Handle {
	        site: 213,
	        target: 163,
	        container: 255,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Unknown(
	                        255,
	                    ),
	                ),
	                bool: true,
	                key: 4294967295,
	                pos: 67108863,
	                length: 6599345300442185728,
	                prop: 3040456637883088859,
	            },
	        ),
	    },
	    SyncAll,
	    Checkout {
	        site: 65,
	        to: 0,
	    },
	    SyncAll,
	    SyncAll,
	    Handle {
	        site: 0,
	        target: 0,
	        container: 131,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    MovableList,
	                ),
	                bool: true,
	                key: 4294967295,
	                pos: 18446744073709551615,
	                length: 2242546323825885183,
	                prop: 2242545357980376863,
	            },
	        ),
	    },
	    Handle {
	        site: 255,
	        target: 255,
	        container: 255,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Text,
	                ),
	                bool: true,
	                key: 4294967049,
	                pos: 1310719,
	                length: 18446744073575268352,
	                prop: 1729382256910270463,
	            },
	        ),
	    },
	    SyncAll,
	    Undo {
	        site: 31,
	        op_len: 2176287547,
	    },
	    Handle {
	        site: 31,
	        target: 31,
	        container: 31,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Tree,
	                ),
	                bool: true,
	                key: 792822677,
	                pos: 9511556229955321855,
	                length: 18446744069951455023,
	                prop: 18446744073709551615,
	            },
	        ),
	    },
	    SyncAll,
	    SyncAll,
	    Handle {
	        site: 31,
	        target: 31,
	        container: 31,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    527965983,
	                ),
	                bool: true,
	                key: 4294967295,
	                pos: 651062616248025087,
	                length: 17870283321406127881,
	                prop: 18446744073709551615,
	            },
	        ),
	    },
	    Sync {
	        from: 163,
	        to: 255,
	    },
	    Handle {
	        site: 31,
	        target: 120,
	        container: 31,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Text,
	                ),
	                bool: true,
	                key: 522133279,
	                pos: 10779248702831402783,
	                length: 9485706711646962581,
	                prop: 2305843005721226239,
	            },
	        ),
	    },
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    Handle {
	        site: 31,
	        target: 31,
	        container: 31,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    522133279,
	                ),
	                bool: false,
	                key: 4294967295,
	                pos: 18446744073709551615,
	                length: 18446744073709551615,
	                prop: 2242545357980376863,
	            },
	        ),
	    },
	    Handle {
	        site: 120,
	        target: 31,
	        container: 31,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Counter,
	                ),
	                bool: true,
	                key: 4294967295,
	                pos: 18446744073709027327,
	                length: 15355022929519706111,
	                prop: 18446744073709551523,
	            },
	        ),
	    },
	    SyncAll,
	    SyncAll,
	    Handle {
	        site: 0,
	        target: 0,
	        container: 0,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Unknown(
	                        255,
	                    ),
	                ),
	                bool: true,
	                key: 707911478,
	                pos: 18446744073709551607,
	                length: 9583660007048690651,
	                prop: 18446744073564528789,
	            },
	        ),
	    },
	    Handle {
	        site: 0,
	        target: 0,
	        container: 0,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Counter,
	                ),
	                bool: true,
	                key: 4294967295,
	                pos: 18446744073709551615,
	                length: 2242792614430507007,
	                prop: 10778762209893752607,
	            },
	        ),
	    },
	    SyncAllUndo {
	        site: 65,
	        op_len: 2751463215,
	    },
	    SyncAllUndo {
	        site: 47,
	        op_len: 4280287231,
	    },
	    Handle {
	        site: 1,
	        target: 4,
	        container: 255,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Unknown(
	                        255,
	                    ),
	                ),
	                bool: true,
	                key: 255,
	                pos: 18446743004262694912,
	                length: 2387225703656530431,
	                prop: 18446744035610665249,
	            },
	        ),
	    },
	    SyncAll,
	    SyncAll,
	    Handle {
	        site: 213,
	        target: 163,
	        container: 255,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    527965983,
	                ),
	                bool: true,
	                key: 4286691203,
	                pos: 2242545357980376863,
	                length: 10779248702831402783,
	                prop: 9485706711646962581,
	            },
	        ),
	    },
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    Handle {
	        site: 31,
	        target: 31,
	        container: 31,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    -1,
	                ),
	                bool: true,
	                key: 4290772991,
	                pos: 18444492273895866367,
	                length: 18446744073709551615,
	                prop: 18446743677852712959,
	            },
	        ),
	    },
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    Handle {
	        site: 0,
	        target: 0,
	        container: 0,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Unknown(
	                        255,
	                    ),
	                ),
	                bool: true,
	                key: 4146737631,
	                pos: 15852670688344145919,
	                length: 10774017683553796411,
	                prop: 18446744073708985120,
	            },
	        ),
	    },
	    Handle {
	        site: 0,
	        target: 0,
	        container: 131,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    MovableList,
	                ),
	                bool: true,
	                key: 654311423,
	                pos: 18446744073709551615,
	                length: 2242792614430507007,
	                prop: 2242545357980376863,
	            },
	        ),
	    },
	    Handle {
	        site: 31,
	        target: 255,
	        container: 255,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Text,
	                ),
	                bool: true,
	                key: 4294904073,
	                pos: 335544319,
	                length: 18446744039333036032,
	                prop: 18446744073709551615,
	            },
	        ),
	    },
	    SyncAll,
	    Handle {
	        site: 120,
	        target: 31,
	        container: 59,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Text,
	                ),
	                bool: true,
	                key: 522133279,
	                pos: 10778687951896697631,
	                length: 18386970223563456899,
	                prop: 18383693675428577237,
	            },
	        ),
	    },
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    Handle {
	        site: 31,
	        target: 31,
	        container: 31,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    2015305503,
	                ),
	                bool: true,
	                key: 4294967071,
	                pos: 651333096108457983,
	                length: 18446744073709488393,
	                prop: 18446744073709551607,
	            },
	        ),
	    },
	    Handle {
	        site: 213,
	        target: 163,
	        container: 255,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    527965983,
	                ),
	                bool: true,
	                key: 4286691203,
	                pos: 2242545357980376863,
	                length: 10779248702831402783,
	                prop: 9485706711646962581,
	            },
	        ),
	    },
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    Handle {
	        site: 31,
	        target: 31,
	        container: 31,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    -8904929,
	                ),
	                bool: true,
	                key: 4294967295,
	                pos: 18446744035054845951,
	                length: 2242792614430507007,
	                prop: 2242545357980376863,
	            },
	        ),
	    },
	    Handle {
	        site: 31,
	        target: 255,
	        container: 255,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Unknown(
	                        255,
	                    ),
	                ),
	                bool: true,
	                key: 4160749567,
	                pos: 18446744073709551615,
	                length: 18446642734358855679,
	                prop: 18446744073709551615,
	            },
	        ),
	    },
	    SyncAll,
	    Handle {
	        site: 0,
	        target: 0,
	        container: 0,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Text,
	                ),
	                bool: false,
	                key: 889126912,
	                pos: 18446744073561321951,
	                length: 71725349863423,
	                prop: 18444310994424758272,
	            },
	        ),
	    },
	    SyncAll,
	    Handle {
	        site: 0,
	        target: 131,
	        container: 131,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    -8398026,
	                ),
	                bool: true,
	                key: 4294967295,
	                pos: 18446744073709551615,
	                length: 2242545361753210879,
	                prop: 2242545357980376863,
	            },
	        ),
	    },
	    SyncAll,
	    SyncAll,
	    Handle {
	        site: 9,
	        target: 9,
	        container: 255,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Text,
	                ),
	                bool: false,
	                key: 4278190080,
	                pos: 18446744073709551607,
	                length: 18420801199931391999,
	                prop: 2267596630907682815,
	            },
	        ),
	    },
	    SyncAll,
	    Handle {
	        site: 31,
	        target: 31,
	        container: 31,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Tree,
	                ),
	                bool: true,
	                key: 4281287043,
	                pos: 3423861436305875967,
	                length: 18446744073694871551,
	                prop: 18446744073709551615,
	            },
	        ),
	    },
	    SyncAll,
	    SyncAll,
	    Handle {
	        site: 31,
	        target: 31,
	        container: 31,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    522156063,
	                ),
	                bool: true,
	                key: 0,
	                pos: 2242546323809107968,
	                length: 10778685111367573279,
	                prop: 18446744073702577559,
	            },
	        ),
	    },
	    Handle {
	        site: 31,
	        target: 31,
	        container: 31,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    522133279,
	                ),
	                bool: true,
	                key: 4280229752,
	                pos: 18446744073709551615,
	                length: 9481649068780656091,
	                prop: 2242545357995061057,
	            },
	        ),
	    },
	    SyncAll,
	    SyncAll,
	    Handle {
	        site: 9,
	        target: 9,
	        container: 255,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Text,
	                ),
	                bool: false,
	                key: 4278190080,
	                pos: 18446744073709551607,
	                length: 18420801199931391999,
	                prop: 2267596630907682815,
	            },
	        ),
	    },
	    SyncAll,
	    Handle {
	        site: 0,
	        target: 0,
	        container: 0,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Unknown(
	                        255,
	                    ),
	                ),
	                bool: true,
	                key: 4294952192,
	                pos: 231945011199,
	                length: 72057594037863685,
	                prop: 6293595036906946614,
	            },
	        ),
	    },
	    Handle {
	        site: 5,
	        target: 5,
	        container: 5,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    84215045,
	                ),
	                bool: true,
	                key: 84215045,
	                pos: 2242545357980391173,
	                length: 10922800942115921695,
	                prop: 11817444525671159189,
	            },
	        ),
	    },
	    SyncAllUndo {
	        site: 47,
	        op_len: 4280287231,
	    },
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    Undo {
	        site: 87,
	        op_len: 1465341783,
	    },
	    Undo {
	        site: 5,
	        op_len: 84215045,
	    },
	    Handle {
	        site: 5,
	        target: 5,
	        container: 5,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    84215045,
	                ),
	                bool: true,
	                key: 84215045,
	                pos: 361700864190383365,
	                length: 361700864190383365,
	                prop: 217020505582416945,
	            },
	        ),
	    },
	    Handle {
	        site: 3,
	        target: 255,
	        container: 255,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    771686497,
	                ),
	                bool: true,
	                key: 1633771873,
	                pos: 3532235001775005793,
	                length: 3314368269007533876,
	                prop: 2242545357984719201,
	            },
	        ),
	    },
	    Handle {
	        site: 219,
	        target: 169,
	        container: 91,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    -42243,
	                ),
	                bool: true,
	                key: 4278190079,
	                pos: 651052016268738559,
	                length: 18377384225446139657,
	                prop: 18446744073709551615,
	            },
	        ),
	    },
	    Handle {
	        site: 9,
	        target: 9,
	        container: 9,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Unknown(
	                        255,
	                    ),
	                ),
	                bool: true,
	                key: 3,
	                pos: 8863084062370168832,
	                length: 18446499982128185343,
	                prop: 18446744073709551615,
	            },
	        ),
	    },
	    Sync {
	        from: 177,
	        to: 177,
	    },
	    Sync {
	        from: 173,
	        to: 173,
	    },
	    Sync {
	        from: 177,
	        to: 177,
	    },
	    Handle {
	        site: 33,
	        target: 0,
	        container: 0,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Unknown(
	                        95,
	                    ),
	                ),
	                bool: true,
	                key: 1600085855,
	                pos: 6872316419617283935,
	                length: 6872316419617283935,
	                prop: 6872316419617283935,
	            },
	        ),
	    },
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    SyncAllUndo {
	        site: 34,
	        op_len: 4280481594,
	    },
	    SyncAll,
	    SyncAll,
	    SyncAllUndo {
	        site: 219,
	        op_len: 1004219995,
	    },
	    Checkout {
	        site: 247,
	        to: 4294901794,
	    },
	    SyncAll,
	    Undo {
	        site: 87,
	        op_len: 3683997527,
	    },
	    SyncAllUndo {
	        site: 34,
	        op_len: 3684040695,
	    },
	    Handle {
	        site: 5,
	        target: 5,
	        container: 5,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    1600085855,
	                ),
	                bool: true,
	                key: 1600085855,
	                pos: 6872316419617283935,
	                length: 6872316419617283935,
	                prop: 6872316419617283935,
	            },
	        ),
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
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    SyncAllUndo {
	        site: 34,
	        op_len: 4280481594,
	    },
	    SyncAll,
	    SyncAll,
	    SyncAllUndo {
	        site: 219,
	        op_len: 1004219995,
	    },
	    Checkout {
	        site: 247,
	        to: 4294901794,
	    },
	    SyncAll,
	    Checkout {
	        site: 219,
	        to: 3141218360,
	    },
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    SyncAllUndo {
	        site: 255,
	        op_len: 1469749914,
	    },
	    Undo {
	        site: 5,
	        op_len: 84215045,
	    },
	    Handle {
	        site: 5,
	        target: 5,
	        container: 5,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    84215045,
	                ),
	                bool: true,
	                key: 84215045,
	                pos: 361700864190383365,
	                length: 361700864190383365,
	                prop: 217020505582416945,
	            },
	        ),
	    },
	    Handle {
	        site: 3,
	        target: 255,
	        container: 255,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    771686497,
	                ),
	                bool: true,
	                key: 1633771873,
	                pos: 3532235001775005793,
	                length: 3314368269007533876,
	                prop: 2242545357984719201,
	            },
	        ),
	    },
	    Handle {
	        site: 219,
	        target: 169,
	        container: 91,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    -42243,
	                ),
	                bool: true,
	                key: 4278190079,
	                pos: 651052016268738559,
	                length: 18377384225446139657,
	                prop: 18446744073709551615,
	            },
	        ),
	    },
	    Handle {
	        site: 9,
	        target: 9,
	        container: 9,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Unknown(
	                        255,
	                    ),
	                ),
	                bool: true,
	                key: 3,
	                pos: 8863084062370168832,
	                length: 18446499982128185343,
	                prop: 18446744073709551615,
	            },
	        ),
	    },
	    Sync {
	        from: 177,
	        to: 177,
	    },
	    Sync {
	        from: 173,
	        to: 173,
	    },
	    Sync {
	        from: 177,
	        to: 177,
	    },
	    Handle {
	        site: 33,
	        target: 0,
	        container: 0,
	        action: Generic(
	            GenericAction {
	                value: Container(
	                    Unknown(
	                        95,
	                    ),
	                ),
	                bool: true,
	                key: 1600085855,
	                pos: 6872316419617283935,
	                length: 6872316419617283935,
	                prop: 6872316419617283935,
	            },
	        ),
	    },
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    SyncAllUndo {
	        site: 34,
	        op_len: 4280481594,
	    },
	    SyncAll,
	    SyncAll,
	    SyncAllUndo {
	        site: 219,
	        op_len: 1004219995,
	    },
	    Checkout {
	        site: 247,
	        to: 4294901794,
	    },
	    SyncAll,
	    Undo {
	        site: 87,
	        op_len: 3683997527,
	    },
	    SyncAllUndo {
	        site: 34,
	        op_len: 3684040695,
	    },
	    Handle {
	        site: 5,
	        target: 5,
	        container: 5,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    1600085855,
	                ),
	                bool: true,
	                key: 1600085855,
	                pos: 6872316419617283935,
	                length: 6872316419617283935,
	                prop: 6872316419617283935,
	            },
	        ),
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
	    SyncAll,
	    SyncAll,
	    Handle {
	        site: 9,
	        target: 9,
	        container: 9,
	        action: Generic(
	            GenericAction {
	                value: I32(
	                    757926153,
	                ),
	                bool: true,
	                key: 1463495981,
	                pos: 6871327012255237381,
	                length: 6872316419617283935,
	                prop: 6872316419617283935,
	            },
	        ),
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
	        op_len: 2509594463,
	    },
	    Undo {
	        site: 126,
	        op_len: 3676700672,
	    },
	    SyncAll,
	    Checkout {
	        site: 223,
	        to: 4287993237,
	    },
	    SyncAll,
	    SyncAllUndo {
	        site: 131,
	        op_len: 586627618,
	    },
	    SyncAll,
	    SyncAll,
	    SyncAll,
	    Undo {
	        site: 219,
	        op_len: 4294967099,
	    },
	    SyncAll,
	    Undo {
	        site: 255,
	        op_len: 3080191,
	    },
	],
    )
}
