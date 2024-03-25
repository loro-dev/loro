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
                    value: I32(993737531),
                    bool: true,
                    key: 2248146944,
                    pos: 254396807995,
                    length: 4268070197446523737,
                    prop: 18446744073696656187,
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
                    value: I32(989867520),
                    bool: true,
                    key: 993737531,
                    pos: 4323455642275625787,
                    length: 254396807995,
                    prop: 18446742995672760320,
                }),
            },
            Sync { from: 139, to: 139 },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 0,
                    pos: 0,
                    length: 4268070196473445179,
                    prop: 4268070196455800882,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 65535,
                    pos: 4268071042544893952,
                    length: 18,
                    prop: 4268071042561343487,
                }),
            },
            Handle {
                site: 0,
                target: 59,
                container: 42,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: false,
                    key: 0,
                    pos: 4268070197442641920,
                    length: 15163,
                    prop: 18446744070085608704,
                }),
            },
            Sync { from: 139, to: 139 },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 0,
                    pos: 721420288,
                    length: 4268070196473445179,
                    prop: 4268070196455800882,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(993737531),
                    bool: false,
                    key: 0,
                    pos: 10088063165293461504,
                    length: 35,
                    prop: 10088063161035600383,
                }),
            },
            Handle {
                site: 255,
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
fn test_movable_list_12() {
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
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(-1),
                    bool: false,
                    key: 4294934527,
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
                    key: 1929430318,
                    pos: 3617009275739948403,
                    length: 32213981296852992,
                    prop: 15553079759578595328,
                }),
            },
            Sync { from: 211, to: 59 },
            Checkout {
                site: 215,
                to: 1004001063,
            },
            Handle {
                site: 0,
                target: 0,
                container: 220,
                action: Generic(GenericAction {
                    value: I32(96),
                    bool: false,
                    key: 2231369728,
                    pos: 9600413840299196417,
                    length: 46059242167205892,
                    prop: 18446744073709551575,
                }),
            },
            Handle {
                site: 0,
                target: 215,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 4294901817,
                    pos: 18446744073709551615,
                    length: 4123390155739970047,
                    prop: 18390793471196266041,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(965082880),
                    bool: true,
                    key: 960051513,
                    pos: 9983072998996785465,
                    length: 18446744073709551498,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 57,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(-65536),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 4121983323008344063,
                    prop: 18374749397238495545,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 57,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(-50887),
                    bool: true,
                    key: 3750179,
                    pos: 18446744073709551615,
                    length: 4123367861550841855,
                    prop: 4123389851770370361,
                }),
            },
            Handle {
                site: 111,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(960051513),
                    bool: true,
                    key: 4280891691,
                    pos: 4179121897159080249,
                    length: 2538122782935628286,
                    prop: 18446744073692789049,
                }),
            },
            Handle {
                site: 0,
                target: 254,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: false,
                    key: 4294967040,
                    pos: 18446744069414584320,
                    length: 4123389851770370559,
                    prop: 4123389851686534713,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 57,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(943270713),
                    bool: true,
                    key: 4294967295,
                    pos: 4268222525080978885,
                    length: 18446743228594731835,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 0,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(993737531),
                    bool: true,
                    key: 1006582587,
                    pos: 10173452862450645819,
                    length: 18446744073709501325,
                    prop: 4268286546840387583,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(993737531),
                    bool: true,
                    key: 3318037307,
                    pos: 55501997373507013,
                    length: 14251014049101083507,
                    prop: 4268070199770858949,
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

#[test]
fn test_movable_list_13() {
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
                site: 164,
                target: 164,
                container: 164,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: false,
                    key: 2762253476,
                    pos: 18446744069677032612,
                    length: 9639893187170402303,
                    prop: 14395694392065640391,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Sync { from: 199, to: 199 },
            SyncAll,
            Handle {
                site: 199,
                target: 199,
                container: 199,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 3351758631,
                    pos: 18446682501058396045,
                    length: 14377039454378393599,
                    prop: 14395693703287523271,
                }),
            },
            SyncAll,
            Handle {
                site: 199,
                target: 199,
                container: 199,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 889192447,
                    pos: 2794421754670843265,
                    length: 14395518472916813767,
                    prop: 14357413797944608711,
                }),
            },
            SyncAll,
            SyncAll,
            Sync { from: 133, to: 199 },
            Handle {
                site: 199,
                target: 199,
                container: 199,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 3351741749,
                    pos: 14395516686210401735,
                    length: 10216353937893083079,
                    prop: 18446743833191383039,
                }),
            },
            Sync { from: 133, to: 199 },
            Handle {
                site: 199,
                target: 199,
                container: 199,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 1070057415,
                    pos: 71610056835194,
                    length: 0,
                    prop: 14395621827009824512,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Sync { from: 133, to: 199 },
            Handle {
                site: 199,
                target: 199,
                container: 199,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 1094698951,
                    pos: 14398519423411421306,
                    length: 14395621199944599495,
                    prop: 14395516686210401735,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 129,
                target: 133,
                container: 199,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 3351717831,
                    pos: 18446744070954403783,
                    length: 18446744073709486134,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 0,
                target: 4,
                container: 33,
                action: Generic(GenericAction {
                    value: I32(-774778624),
                    bool: true,
                    key: 2051096519,
                    pos: 15069330226212913408,
                    length: 9598797841674258385,
                    prop: 2749385757289859015,
                }),
            },
            SyncAll,
            Sync { from: 255, to: 255 },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 4,
                target: 33,
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
fn test_movable_list_14() {
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
                site: 33,
                target: 209,
                container: 197,
                action: Generic(GenericAction {
                    value: I32(-1179010631),
                    bool: true,
                    key: 3115956665,
                    pos: 13382931975044184505,
                    length: 13382931975044184505,
                    prop: 13382931975044184505,
                }),
            },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Checkout {
                site: 45,
                to: 2147483629,
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 45,
                target: 229,
                container: 229,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 3537031981,
                    pos: 13382910049657213650,
                    length: 562949940558265,
                    prop: 18446744073709486336,
                }),
            },
            SyncAll,
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(-1),
                    bool: true,
                    key: 991640575,
                    pos: 4268070197446523707,
                    length: 65125582846779,
                    prop: 18446743228596882176,
                }),
            },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Handle {
                site: 237,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 757982679,
                    pos: 16565899576820444512,
                    length: 2378153378818021335,
                    prop: 14470860355616821971,
                }),
            },
            Sync { from: 59, to: 255 },
            Handle {
                site: 185,
                target: 185,
                container: 185,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 3115956665,
                    pos: 13382931975044184505,
                    length: 13382931975044184505,
                    prop: 13382931975044184505,
                }),
            },
            SyncAll,
            Handle {
                site: 229,
                target: 229,
                container: 229,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 3857001773,
                    pos: 16565899519554217445,
                    length: 15191436295996124461,
                    prop: 4303675126263957714,
                }),
            },
            Handle {
                site: 0,
                target: 1,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 1006632959,
                    pos: 4268070197446523707,
                    length: 18446744073709551361,
                    prop: 4268286546840387583,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 993737531,
                    pos: 4268070197446523707,
                    length: 4277305840723049275,
                    prop: 72057602627862331,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(-197),
                    bool: true,
                    key: 456916991,
                    pos: 4268070197446523707,
                    length: 16672149208775483,
                    prop: 18446744073709551360,
                }),
            },
            SyncAll,
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: false,
                    key: 4294902016,
                    pos: 18446744073709551615,
                    length: 4268070197459419135,
                    prop: 18446527724320078651,
                }),
            },
            Handle {
                site: 27,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(993737531),
                    bool: true,
                    key: 993737531,
                    pos: 18391358769805131776,
                    length: 13382728867219898367,
                    prop: 13382931975044184505,
                }),
            },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            SyncAll,
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(993737531),
                    bool: true,
                    key: 184549376,
                    pos: 4991460930710354780,
                    length: 4991471925827290437,
                    prop: 4268070197446523707,
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
                    length: 0,
                    prop: 0,
                }),
            },
        ],
    )
}

#[test]
fn test_movable_list_15() {
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
                    pos: 17578395995350237183,
                    length: 4268273300959130611,
                    prop: 4251398048275184443,
                }),
            },
            Sync { from: 59, to: 59 },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(-50373),
                    bool: true,
                    key: 4294967295,
                    pos: 3314707854257765179,
                    length: 18446527724315687680,
                    prop: 7306357456639098880,
                }),
            },
            Checkout {
                site: 101,
                to: 1701143909,
            },
            Checkout {
                site: 101,
                to: 838860800,
            },
            Handle {
                site: 101,
                target: 101,
                container: 101,
                action: Generic(GenericAction {
                    value: I32(1701143909),
                    bool: true,
                    key: 1701143909,
                    pos: 7306357456645743973,
                    length: 18446744073693102080,
                    prop: 4268070197442772991,
                }),
            },
            Handle {
                site: 0,
                target: 59,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(993737472),
                    bool: true,
                    key: 16777019,
                    pos: 7306357456645718016,
                    length: 7306357456645743973,
                    prop: 7306357456645743973,
                }),
            },
            Checkout {
                site: 101,
                to: 1701143909,
            },
            Checkout {
                site: 101,
                to: 973078405,
            },
            Handle {
                site: 57,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(960051513),
                    bool: true,
                    key: 2215329547,
                    pos: 18446744073709551527,
                    length: 18446525516668811577,
                    prop: 17578661999653421055,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(15163),
                    bool: false,
                    key: 993737606,
                    pos: 4268070197448474624,
                    length: 18446743228594731835,
                    prop: 4268070200747753471,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: Container(Map),
                    bool: true,
                    key: 1701143909,
                    pos: 65306678879589,
                    length: 18446742995672760320,
                    prop: 18391573887352569855,
                }),
            },
        ],
    )
}

#[test]
fn test_movable_list_16() {
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
                    prop: 18446744073696656187,
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
                    value: I32(989867520),
                    bool: true,
                    key: 993737531,
                    pos: 4323455642275625787,
                    length: 254396807995,
                    prop: 18446742995672760320,
                }),
            },
            Sync { from: 139, to: 139 },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 0,
                    pos: 0,
                    length: 4268070196473445179,
                    prop: 4268070196455800882,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 50529279,
                    pos: 217064709432738563,
                    length: 1095216661759,
                    prop: 4268070200747689216,
                }),
            },
            SyncAll,
            Handle {
                site: 59,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 0,
                    pos: 0,
                    length: 16672149208775483,
                    prop: 18376093854555176960,
                }),
            },
            SyncAll,
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(-50373),
                    bool: true,
                    key: 993787903,
                    pos: 18446744073696656187,
                    length: 14138873509707775,
                    prop: 18446616030185455662,
                }),
            },
        ],
    )
}

#[test]
fn test_movable_list_17() {
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
                site: 41,
                target: 34,
                container: 115,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 1111621632,
                    pos: 4774451407313060418,
                    length: 3242591731706774082,
                    prop: 16565696476406951213,
                }),
            },
            Handle {
                site: 0,
                target: 229,
                container: 45,
                action: Generic(GenericAction {
                    value: I32(757935365),
                    bool: true,
                    key: 3621250533,
                    pos: 3255307777725556197,
                    length: 9596332072585997613,
                    prop: 3288537597569377199,
                }),
            },
            Handle {
                site: 2,
                target: 192,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-256),
                    bool: false,
                    key: 4294901760,
                    pos: 18446597888123012403,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            Checkout {
                site: 38,
                to: 4294960036,
            },
            Sync { from: 11, to: 123 },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(771650308),
                    bool: true,
                    key: 3772157069,
                    pos: 4268070199141008499,
                    length: 8303295463570488123,
                    prop: 4268070197446549508,
                }),
            },
            Handle {
                site: 18,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(993737531),
                    bool: true,
                    key: 993737531,
                    pos: 4774378554970028859,
                    length: 4774451407313060418,
                    prop: 3242591731706774082,
                }),
            },
            Handle {
                site: 45,
                target: 229,
                container: 229,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 1944114828,
                    pos: 4268070197453142788,
                    length: 320665124056283963,
                    prop: 4268070197446523808,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(993737531),
                    bool: true,
                    key: 993737531,
                    pos: 4774451122733595451,
                    length: 4774451407313060418,
                    prop: 3255258105658736706,
                }),
            },
        ],
    )
}

#[test]
fn test_movable_list_18() {
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
                site: 41,
                target: 34,
                container: 115,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 1111621632,
                    pos: 4774451407313060418,
                    length: 3242591731706774082,
                    prop: 16565696476406951213,
                }),
            },
            Handle {
                site: 0,
                target: 229,
                container: 45,
                action: Generic(GenericAction {
                    value: I32(757935365),
                    bool: true,
                    key: 3621250533,
                    pos: 3255307777725556197,
                    length: 9596332072585997613,
                    prop: 3288537597569377199,
                }),
            },
            Handle {
                site: 2,
                target: 192,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-256),
                    bool: false,
                    key: 4294901760,
                    pos: 18446597888123012403,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            Checkout {
                site: 38,
                to: 4294960036,
            },
            Sync { from: 11, to: 123 },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(771650308),
                    bool: true,
                    key: 3772157069,
                    pos: 4268070199141008499,
                    length: 8303295463570488123,
                    prop: 4268070197446549508,
                }),
            },
            Handle {
                site: 18,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(993737531),
                    bool: true,
                    key: 993737531,
                    pos: 4774378554970028859,
                    length: 4774451407313060418,
                    prop: 3242591731706774082,
                }),
            },
            Handle {
                site: 45,
                target: 229,
                container: 229,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 1944114828,
                    pos: 4268070197453142788,
                    length: 320665124056283963,
                    prop: 4268070197446523808,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(993737531),
                    bool: true,
                    key: 993737531,
                    pos: 4774451122733595451,
                    length: 4774451407313060418,
                    prop: 3255258105658736706,
                }),
            },
            SyncAll,
            SyncAll,
            Sync { from: 227, to: 255 },
            Handle {
                site: 132,
                target: 11,
                container: 123,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 0,
                    pos: 3314212838480740352,
                    length: 4252651357653994811,
                    prop: 4268070197446523808,
                }),
            },
            Checkout {
                site: 4,
                to: 992361376,
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(993737531),
                    bool: true,
                    key: 993737531,
                    pos: 4268070197446523707,
                    length: 4268070197446523707,
                    prop: 4268070197446523707,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(-475781517),
                    bool: true,
                    key: 187957247,
                    pos: 18446743470183515147,
                    length: 34359738367,
                    prop: 18335003334139707392,
                }),
            },
            SyncAll,
        ],
    )
}

#[test]
fn test_movable_list_19() {
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
                site: 33,
                target: 209,
                container: 197,
                action: Generic(GenericAction {
                    value: I32(129612217),
                    bool: true,
                    key: 4294967295,
                    pos: 13382931975044202495,
                    length: 13382931975044184505,
                    prop: 13382931975044184505,
                }),
            },
            Sync { from: 185, to: 185 },
            Checkout {
                site: 45,
                to: 2147483629,
            },
            SyncAll,
            Handle {
                site: 45,
                target: 229,
                container: 229,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 3537031981,
                    pos: 13382910049657213650,
                    length: 562949940558265,
                    prop: 18446744073709486336,
                }),
            },
            SyncAll,
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(-1),
                    bool: true,
                    key: 991640575,
                    pos: 4268070197446523707,
                    length: 2305908134796540731,
                    prop: 18446743228596882176,
                }),
            },
            Sync { from: 185, to: 13 },
            Handle {
                site: 237,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 757982679,
                    pos: 16565899576820444461,
                    length: 2442610164937250775,
                    prop: 14470860355616821971,
                }),
            },
            Sync { from: 59, to: 255 },
            Handle {
                site: 185,
                target: 185,
                container: 185,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 3115956665,
                    pos: 13382931975044184505,
                    length: 13382931975044184505,
                    prop: 13382931975044184505,
                }),
            },
            SyncAll,
            Handle {
                site: 229,
                target: 229,
                container: 229,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 757982679,
                    pos: 16565899576820444461,
                    length: 2378153378818021335,
                    prop: 14470860355616821971,
                }),
            },
            Sync { from: 59, to: 255 },
            Handle {
                site: 185,
                target: 185,
                container: 185,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 3115956665,
                    pos: 13382931975044184505,
                    length: 13382931975044184505,
                    prop: 13382931975044184505,
                }),
            },
            SyncAll,
            Handle {
                site: 229,
                target: 229,
                container: 229,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 3857001773,
                    pos: 16565899519554217445,
                    length: 15191436295996124461,
                    prop: 4303675126263957714,
                }),
            },
            Handle {
                site: 0,
                target: 1,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 1006632959,
                    pos: 4268070197446523707,
                    length: 18446744073709551361,
                    prop: 4268286546840387583,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 993737727,
                    pos: 18391358628880399163,
                    length: 4259063843306602495,
                    prop: 4268070197446523707,
                }),
            },
            Handle {
                site: 59,
                target: 0,
                container: 23,
                action: Generic(GenericAction {
                    value: I32(-197),
                    bool: true,
                    key: 3103850496,
                    pos: 13382931975044184505,
                    length: 13382931975044184505,
                    prop: 13382931975035730361,
                }),
            },
            SyncAll,
            Handle {
                site: 229,
                target: 229,
                container: 229,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 3857001773,
                    pos: 16565899519554217445,
                    length: 15191436295996121344,
                    prop: 4303675126263957714,
                }),
            },
            Handle {
                site: 0,
                target: 185,
                container: 185,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 3115956665,
                    pos: 13382931975044184505,
                    length: 13382931975044184505,
                    prop: 13382931975044184505,
                }),
            },
            Handle {
                site: 237,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 757982679,
                    pos: 16565899576820444461,
                    length: 3255510881225136087,
                    prop: 14470860355616821971,
                }),
            },
            Sync { from: 59, to: 255 },
            Handle {
                site: 1,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 993787903,
                    pos: 88729743246703419,
                    length: 18446744073709551615,
                    prop: 4268071042561343487,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Map),
                    bool: true,
                    key: 993737531,
                    pos: 4268070197446523707,
                    length: 4268106274178072635,
                    prop: 18374967954681888767,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(-1),
                    bool: true,
                    key: 991640575,
                    pos: 4268070197446523707,
                    length: 65125582846779,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: false,
                    key: 4294967041,
                    pos: 18446744073709551615,
                    length: 4268070196909703167,
                    prop: 4268070197446523707,
                }),
            },
            Handle {
                site: 0,
                target: 11,
                container: 92,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 4268070197446523903,
                    prop: 18446744073696656187,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(993737531),
                    bool: true,
                    key: 4278190139,
                    pos: 18446744073709551615,
                    length: 4268070200747753471,
                    prop: 18446527724315687739,
                }),
            },
            Handle {
                site: 1,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 993787903,
                    pos: 4268143864725584699,
                    length: 4323455642275675963,
                    prop: 4268070197446523675,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(-12895396),
                    bool: true,
                    key: 511,
                    pos: 13382931975044184321,
                    length: 13382931975044184505,
                    prop: 13382931975044184505,
                }),
            },
            Sync { from: 185, to: 185 },
            SyncAll,
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(993737531),
                    bool: true,
                    key: 59,
                    pos: 4988657175891762187,
                    length: 4991471925827290437,
                    prop: 4268070197446523717,
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
                    length: 0,
                    prop: 0,
                }),
            },
        ],
    )
}
