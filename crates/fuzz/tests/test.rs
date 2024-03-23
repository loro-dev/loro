use fuzz::{
    actions::{ActionInner, ActionWrapper::*, GenericAction, MovableListAction},
    crdt_fuzzer::{test_multi_sites, Action::*, FuzzTarget, FuzzValue::*},
};
use loro::ContainerType::*;

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

#[test]
fn test_movable_list_0() {
    test_multi_sites(
        2,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 117,
                target: 166,
                container: 10,
                action: Generic(GenericAction {
                    value: I32(-273622840),
                    bool: false,
                    key: 2741083633,
                    pos: 6666897757659758022,
                    length: 8533446734363315434,
                    prop: 12864568433311511070,
                }),
            },
            Handle {
                site: 124,
                target: 14,
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

#[test]
fn test_movable_list_1() {
    test_multi_sites(
        2,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 164,
                target: 239,
                container: 61,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 1768571449,
                    pos: 5035915790398973222,
                    length: 12262157610559101276,
                    prop: 2115599344051559167,
                }),
            },
            Sync { from: 172, to: 249 },
            Handle {
                site: 76,
                target: 185,
                container: 213,
                action: Generic(GenericAction {
                    value: I32(-180416322),
                    bool: false,
                    key: 905065406,
                    pos: 13106072747215825198,
                    length: 14041671030581285265,
                    prop: 15938081911894848481,
                }),
            },
        ],
    )
}

#[test]
fn test_movable_list_2() {
    test_multi_sites(
        2,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 44,
                target: 124,
                container: 221,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 3351758791,
                    pos: 288230650086410183,
                    length: 2606365581092837153,
                    prop: 15553136935972341051,
                }),
            },
            SyncAll,
            Handle {
                site: 109,
                target: 209,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(1145324612),
                    bool: true,
                    key: 3351758806,
                    pos: 9187202260886079431,
                    length: 72056541770940543,
                    prop: 70127282814975,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
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
        ],
    )
}

#[test]
fn test_movable_list_3() {
    test_multi_sites(
        2,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(-1),
                    bool: false,
                    key: 4294967295,
                    pos: 18446744073709551419,
                    length: 4268071042561343487,
                    prop: 4268070197446523698,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 3318072622,
                    pos: 14251014049101104581,
                    length: 18391358628880399163,
                    prop: 4268070197442641920,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(1568286093),
                    bool: true,
                    key: 999132557,
                    pos: 216172782113783807,
                    length: 15626148457674914619,
                    prop: 18446693297831399889,
                }),
            },
            SyncAll,
            Handle {
                site: 92,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(1162167621),
                    bool: true,
                    key: 993737541,
                    pos: 15163,
                    length: 18391358628880386048,
                    prop: 1099511627774,
                }),
            },
        ],
    )
}

#[test]
fn test_movable_list_4() {
    test_multi_sites(
        5,
        vec![
            FuzzTarget::Map,
            FuzzTarget::List,
            FuzzTarget::Text,
            FuzzTarget::Tree,
            FuzzTarget::MovableList,
        ],
        &mut [
            SyncAll,
            Handle {
                site: 91,
                target: 59,
                container: 34,
                action: Generic(GenericAction {
                    value: I32(-2088551680),
                    bool: true,
                    key: 131,
                    pos: 16855269067351588864,
                    length: 6911312294037809641,
                    prop: 16855260268008005471,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 160,
                target: 19,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 930317187,
                    pos: 4251419660595589899,
                    length: 10993036654195,
                    prop: 18446743523953737728,
                }),
            },
            SyncAll,
            Checkout {
                site: 79,
                to: 2147483432,
            },
            Handle {
                site: 34,
                target: 34,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(572662306),
                    bool: false,
                    key: 829760512,
                    pos: 4319796467578386228,
                    length: 18446744073709551615,
                    prop: 2676586395008836901,
                }),
            },
            Handle {
                site: 37,
                target: 37,
                container: 37,
                action: Generic(GenericAction {
                    value: I32(623191333),
                    bool: true,
                    key: 623191333,
                    pos: 2676586395008836901,
                    length: 2676586395008836901,
                    prop: 10455415605503269,
                }),
            },
        ],
    )
}

#[test]
fn missing_event_when_checkout() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Map, FuzzTarget::Tree],
        &mut [
            Handle {
                site: 39,
                target: 39,
                container: 39,
                action: Generic(GenericAction {
                    value: I32(656877351),
                    bool: true,
                    key: 656877351,
                    pos: 2821223700817717031,
                    length: 2821266740684990247,
                    prop: 2821266740684990247,
                }),
            },
            Handle {
                site: 39,
                target: 39,
                container: 39,
                action: Generic(GenericAction {
                    value: I32(656877351),
                    bool: true,
                    key: 656877351,
                    pos: 2821266740684990247,
                    length: 2821266740684990247,
                    prop: 2821266740684990247,
                }),
            },
            Handle {
                site: 39,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Handle {
                site: 39,
                target: 39,
                container: 39,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 1811993856,
                    pos: 1585267068834414592,
                    length: 18389323175239352127,
                    prop: 2745369343,
                }),
            },
        ],
    )
}

#[test]
fn tree_meta() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Map, FuzzTarget::Tree],
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
                    prop: 57140735609148179,
                }),
            },
            Sync { from: 171, to: 139 },
            Handle {
                site: 171,
                target: 171,
                container: 39,
                action: Generic(GenericAction {
                    value: Container(Map),
                    bool: false,
                    key: 2868955905,
                    pos: 1374463283923456787,
                    length: 1374722768667623699,
                    prop: 18446743056122319677,
                }),
            },
            Sync { from: 131, to: 131 },
            Handle {
                site: 19,
                target: 19,
                container: 19,
                action: Generic(GenericAction {
                    value: I32(320017200),
                    bool: false,
                    key: 320541459,
                    pos: 1374463283923456787,
                    length: 1374463283923456787,
                    prop: 1374463283923456787,
                }),
            },
            Handle {
                site: 19,
                target: 19,
                container: 19,
                action: Generic(GenericAction {
                    value: I32(-2088533229),
                    bool: true,
                    key: 2206434179,
                    pos: 9476562641788044153,
                    length: 9476562641653826435,
                    prop: 9511602412998329219,
                }),
            },
            Sync { from: 131, to: 131 },
            Handle {
                site: 19,
                target: 19,
                container: 19,
                action: Generic(GenericAction {
                    value: I32(320028947),
                    bool: true,
                    key: 320017171,
                    pos: 18446744073709490963,
                    length: 18446744073709551615,
                    prop: 1374463283923477011,
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
                    pos: 1374463284779094803,
                    length: 1374463283923456787,
                    prop: 1374463283923981075,
                }),
            },
            Handle {
                site: 19,
                target: 19,
                container: 19,
                action: Generic(GenericAction {
                    value: I32(320017171),
                    bool: true,
                    key: 2206434179,
                    pos: 1374662778450838403,
                    length: 280459318858515,
                    prop: 207235723598,
                }),
            },
        ],
    );
}

#[test]
fn richtext_lamport_issue() {
    test_multi_sites(
        5,
        vec![
            FuzzTarget::Map,
            FuzzTarget::List,
            FuzzTarget::Tree,
            FuzzTarget::Text,
        ],
        &mut [
            Handle {
                site: 196,
                target: 1,
                container: 1,
                action: Generic(GenericAction {
                    value: I32(123),
                    bool: true,
                    key: 16843009,
                    pos: 72340172838076673,
                    length: 72340172838076673,
                    prop: 72340172838076673,
                }),
            },
            Handle {
                site: 1,
                target: 1,
                container: 1,
                action: Generic(GenericAction {
                    value: I32(456),
                    bool: true,
                    key: 4294967041,
                    pos: 18446744073692849663,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Checkout {
                site: 0,
                to: 20587776,
            },
            Handle {
                site: 1,
                target: 1,
                container: 1,
                action: Generic(GenericAction {
                    value: I32(789),
                    bool: true,
                    key: 16843009,
                    pos: 72340172838076673,
                    length: 72340172838076673,
                    prop: 72340172838076673,
                }),
            },
            SyncAll,
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 18446744073038462975,
                    prop: 15925010861198934015,
                }),
            },
        ],
    )
}

#[test]
fn tree_get_child_index() {
    test_multi_sites(
        5,
        vec![
            FuzzTarget::Map,
            FuzzTarget::List,
            FuzzTarget::Tree,
            FuzzTarget::Text,
        ],
        &mut [
            Handle {
                site: 19,
                target: 19,
                container: 19,
                action: Generic(GenericAction {
                    value: I32(320017171),
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
                    prop: 57140735609148179,
                }),
            },
            Sync { from: 171, to: 139 },
            Handle {
                site: 171,
                target: 171,
                container: 40,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: false,
                    key: 6007723,
                    pos: 41377556015874048,
                    length: 41565808209305343,
                    prop: 37687,
                }),
            },
        ],
    )
}

#[test]
fn list_delete_change_to_diff_assert() {
    test_multi_sites(
        5,
        vec![
            FuzzTarget::Map,
            FuzzTarget::List,
            FuzzTarget::Tree,
            FuzzTarget::Text,
        ],
        &mut [
            Handle {
                site: 61,
                target: 61,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(1027423549),
                    bool: true,
                    key: 1027423549,
                    pos: 4412750543122677053,
                    length: 3259829041783373823,
                    prop: 4412187962536443197,
                }),
            },
            Handle {
                site: 61,
                target: 61,
                container: 61,
                action: Generic(GenericAction {
                    value: I32(-12763843),
                    bool: true,
                    key: 1040187391,
                    pos: 4412750543122726717,
                    length: 1845454810429,
                    prop: 4444398755940139008,
                }),
            },
            Handle {
                site: 255,
                target: 59,
                container: 1,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4294967295,
                    pos: 4412750543122726911,
                    length: 9024436561550065151,
                    prop: 3602665457070193981,
                }),
            },
            Handle {
                site: 49,
                target: 49,
                container: 49,
                action: Generic(GenericAction {
                    value: I32(825307441),
                    bool: true,
                    key: 1027423537,
                    pos: 4436957391119789373,
                    length: 18391923786480696635,
                    prop: 4412750543122701885,
                }),
            },
            SyncAll,
            Handle {
                site: 61,
                target: 61,
                container: 61,
                action: Generic(GenericAction {
                    value: I32(1027423549),
                    bool: true,
                    key: 4294967295,
                    pos: 3544668469066546687,
                    length: 3616726063103684913,
                    prop: 18436571237510545407,
                }),
            },
            SyncAll,
            Handle {
                site: 61,
                target: 61,
                container: 61,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: false,
                    key: 2904489984,
                    pos: 18391361205894462893,
                    length: 2654601531,
                    prop: 0,
                }),
            },
        ],
    )
}

#[test]
fn test_movable_list_5() {
    test_multi_sites(
        5,
        vec![
            FuzzTarget::Map,
            FuzzTarget::List,
            FuzzTarget::Text,
            FuzzTarget::Tree,
            FuzzTarget::MovableList,
        ],
        &mut [
            Handle {
                site: 3,
                target: 34,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Map),
                    bool: false,
                    key: 50536963,
                    pos: 217020518514230019,
                    length: 217020518514230019,
                    prop: 217020518514230019,
                }),
            },
            SyncAll,
            Handle {
                site: 3,
                target: 3,
                container: 3,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 4294967295,
                    pos: 3399987922982666239,
                    length: 940450980798869287,
                    prop: 5391038347781079093,
                }),
            },
            Checkout {
                site: 3,
                to: 2072347904,
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

#[test]
fn test_movable_list_6() {
    test_multi_sites(
        5,
        vec![
            FuzzTarget::Map,
            FuzzTarget::List,
            FuzzTarget::Text,
            FuzzTarget::Tree,
            FuzzTarget::MovableList,
        ],
        &mut [
            Handle {
                site: 44,
                target: 124,
                container: 221,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: false,
                    key: 38,
                    pos: 150994944,
                    length: 18446742974197923840,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 194,
                target: 239,
                container: 251,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 0,
                    pos: 0,
                    length: 18446608833779269692,
                    prop: 18446744073708503039,
                }),
            },
            Handle {
                site: 0,
                target: 255,
                container: 133,
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

#[test]
fn test_movable_list_7() {
    test_multi_sites(
        5,
        vec![
            FuzzTarget::Map,
            FuzzTarget::List,
            FuzzTarget::Text,
            FuzzTarget::Tree,
            FuzzTarget::MovableList,
        ],
        &mut [
            Handle {
                site: 44,
                target: 124,
                container: 221,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 3351758791,
                    pos: 288230650086410183,
                    length: 2606365581092837153,
                    prop: 15553136935972341051,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 209,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(1145324612),
                    bool: true,
                    key: 3351758806,
                    pos: 9187202260886079431,
                    length: 72056541770940543,
                    prop: 70127282814975,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
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
        ],
    )
}

#[test]
fn test_movable_list_8() {
    test_multi_sites(
        5,
        vec![
            FuzzTarget::Map,
            FuzzTarget::List,
            FuzzTarget::Text,
            FuzzTarget::Tree,
            FuzzTarget::MovableList,
        ],
        &mut [
            Handle {
                site: 3,
                target: 34,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 2582786094,
                    pos: 18374686655815811843,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Handle {
                site: 3,
                target: 133,
                container: 3,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 960051513,
                    pos: 4123389851770370361,
                    length: 4123389851770370361,
                    prop: 4123389851770370361,
                }),
            },
            Handle {
                site: 57,
                target: 59,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(825307441),
                    bool: true,
                    key: 825307441,
                    pos: 3544668469065756977,
                    length: 3544668469065756977,
                    prop: 3544668469065756977,
                }),
            },
            Handle {
                site: 49,
                target: 49,
                container: 49,
                action: Generic(GenericAction {
                    value: I32(960051513),
                    bool: true,
                    key: 960051513,
                    pos: 4123389851770370361,
                    length: 268877889158068537,
                    prop: 253612265486615299,
                }),
            },
            Handle {
                site: 3,
                target: 215,
                container: 213,
                action: Generic(GenericAction {
                    value: I32(3),
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

#[test]
fn test_movable_list_9() {
    test_multi_sites(
        5,
        vec![
            FuzzTarget::Map,
            FuzzTarget::List,
            FuzzTarget::Text,
            FuzzTarget::Tree,
            FuzzTarget::MovableList,
        ],
        &mut [
            Handle {
                site: 1,
                target: 64,
                container: 36,
                action: Generic(GenericAction {
                    value: I32(993737531),
                    bool: true,
                    key: 2248146944,
                    pos: 254396807995,
                    length: 4268070197446523737,
                    prop: 18446744073696655675,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 0,
                    pos: 0,
                    length: 4268070197446523648,
                    prop: 360287970189639680,
                }),
            },
            SyncAll,
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 0,
                    pos: 0,
                    length: 4268070197446523649,
                    prop: 4268070196455800882,
                }),
            },
            SyncAll,
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 4294967045,
                    pos: 18413964932892298239,
                    length: 3746779384955142143,
                    prop: 255,
                }),
            },
        ],
    )
}

#[test]
fn test_movable_list_10() {
    test_multi_sites(
        5,
        vec![
            FuzzTarget::Map,
            FuzzTarget::List,
            FuzzTarget::Text,
            FuzzTarget::Tree,
            FuzzTarget::MovableList,
        ],
        &mut [
            Handle {
                site: 1,
                target: 64,
                container: 36,
                action: Generic(GenericAction {
                    value: I32(989855744),
                    bool: true,
                    key: 2248146944,
                    pos: 4268102928402430779,
                    length: 4268070197446523707,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 4294903040,
                    pos: 4268007270886932479,
                    length: 3314707854257765179,
                    prop: 4268070197446523648,
                }),
            },
            Handle {
                site: 89,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(-281330885),
                    bool: true,
                    key: 4294967099,
                    pos: 13021231110858735615,
                    length: 13021231110853801140,
                    prop: 18425550663698396340,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 4278517760,
                    pos: 2199023255551,
                    length: 13575924464958210,
                    prop: 18444988998762561582,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(993722414),
                    bool: true,
                    key: 4294916923,
                    pos: 7306357456639098880,
                    length: 7306357456645743973,
                    prop: 7306357456645729125,
                }),
            },
            Checkout {
                site: 101,
                to: 1701143909,
            },
            Checkout {
                site: 101,
                to: 1701143909,
            },
            Checkout {
                site: 101,
                to: 25957,
            },
            SyncAll,
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(989867520),
                    bool: false,
                    key: 0,
                    pos: 18446744073709487360,
                    length: 71833290377462271,
                    prop: 0,
                }),
            },
        ],
    )
}

#[test]
fn test_movable_list_11() {
    test_multi_sites(
        5,
        vec![
            FuzzTarget::Map,
            FuzzTarget::List,
            FuzzTarget::Text,
            FuzzTarget::Tree,
            FuzzTarget::MovableList,
        ],
        &mut [
            Handle {
                site: 1,
                target: 64,
                container: 36,
                action: Generic(GenericAction {
                    value: I32(989855744),
                    bool: true,
                    key: 2248146944,
                    pos: 4268102928402430779,
                    length: 4268070197446523707,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 4294903040,
                    pos: 4268007270886932479,
                    length: 3314707854257765179,
                    prop: 4268070197446523648,
                }),
            },
            Handle {
                site: 89,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(1005534011),
                    bool: true,
                    key: 4294967295,
                    pos: 13021231110853820415,
                    length: 13021231110853801140,
                    prop: 18446661286951695540,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 4294903040,
                    pos: 4268007270886932479,
                    length: 3314702356699626299,
                    prop: 18446743228594731776,
                }),
            },
            Sync { from: 163, to: 48 },
            Sync { from: 163, to: 36 },
            SyncAll,
            SyncAll,
            Handle {
                site: 0,
                target: 50,
                container: 133,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: false,
                    key: 4294967190,
                    pos: 4149669093542199295,
                    length: 10824746097297668409,
                    prop: 17578661361369962390,
                }),
            },
            SyncAll,
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(3881728),
                    bool: false,
                    key: 993756672,
                    pos: 4268070197945958459,
                    length: 18446527724315687739,
                    prop: 4289181665814642687,
                }),
            },
            Checkout {
                site: 59,
                to: 993737531,
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Action(ActionInner::MovableList(MovableListAction::Delete {
                    pos: 0,
                    len: 3,
                })),
            },
            Checkout {
                site: 101,
                to: 1701143909,
            },
            Checkout {
                site: 101,
                to: 1701143909,
            },
        ],
    )
}
