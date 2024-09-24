use std::sync::Arc;

use arbitrary::Unstructured;
use fuzz::{
    actions::{
        ActionInner,
        ActionWrapper::{self, *},
        GenericAction,
    },
    container::{MapAction, TextAction, TextActionInner, TreeAction, TreeActionInner},
    crdt_fuzzer::{
        minify_error, test_multi_sites,
        Action::{self, *},
        FuzzTarget,
        FuzzValue::*,
    },
    test_multi_sites_on_one_doc, test_multi_sites_with_gc,
};
use loro::{ContainerType::*, LoroCounter, LoroDoc};

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

#[test]
fn test_empty() {
    test_multi_sites(5, vec![FuzzTarget::All], &mut [])
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
fn delta_err() {
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
                site: 57,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(758724921),
                    bool: false,
                    key: 57,
                    pos: 6655295504416505856,
                    length: 4123474264100363356,
                    prop: 4123389851770370361,
                }),
            },
            Handle {
                site: 57,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(960051513),
                    bool: true,
                    key: 960051513,
                    pos: 4123389851770370361,
                    length: 4123389851770370361,
                    prop: 4123389847475403263,
                }),
            },
            Handle {
                site: 57,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(960051513),
                    bool: true,
                    key: 14649,
                    pos: 6655194349346750464,
                    length: 4144999408248577116,
                    prop: 4123389851770370361,
                }),
            },
            Handle {
                site: 57,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(960051513),
                    bool: true,
                    key: 960051513,
                    pos: 4123389851770370361,
                    length: 4123389851770370361,
                    prop: 4123388752258793273,
                }),
            },
            Handle {
                site: 57,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(960051554),
                    bool: true,
                    key: 960051513,
                    pos: 4123389851770370361,
                    length: 4123389851770370361,
                    prop: 4123108376806635833,
                }),
            },
            Handle {
                site: 57,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(960051513),
                    bool: true,
                    key: 960051513,
                    pos: 0,
                    length: 18374733605602352220,
                    prop: 4123389851770370437,
                }),
            },
            Handle {
                site: 57,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(960061952),
                    bool: true,
                    key: 960051513,
                    pos: 4123389851770370361,
                    length: 4123389851770370361,
                    prop: 4123389855092259129,
                }),
            },
            Handle {
                site: 57,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(960051513),
                    bool: true,
                    key: 960051513,
                    pos: 4179111962746552121,
                    length: 3098478742654093369,
                    prop: 4123389851783332611,
                }),
            },
            Handle {
                site: 57,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(960051513),
                    bool: true,
                    key: 960051513,
                    pos: 3472301697646803343,
                    length: 18446743009517764400,
                    prop: 4123607322237534209,
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
fn delta_err_2() {
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
                site: 133,
                target: 35,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(757935405),
                    bool: true,
                    key: 75181359,
                    pos: 8286624611106291583,
                    length: 18446585211651489792,
                    prop: 6713178190254243839,
                }),
            },
            Checkout {
                site: 223,
                to: 2240120100,
            },
            Handle {
                site: 45,
                target: 255,
                container: 41,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 757935405,
                    pos: 3255307777713450285,
                    length: 3255307777713450285,
                    prop: 3255307777713450285,
                }),
            },
            Handle {
                site: 45,
                target: 45,
                container: 45,
                action: Generic(GenericAction {
                    value: I32(757935405),
                    bool: true,
                    key: 768660269,
                    pos: 3255307777713450285,
                    length: 3255307777713450285,
                    prop: 3255307777713450285,
                }),
            },
            Handle {
                site: 45,
                target: 45,
                container: 45,
                action: Generic(GenericAction {
                    value: I32(-617796307),
                    bool: true,
                    key: 757935405,
                    pos: 3255307777713450285,
                    length: 3255307777722690861,
                    prop: 3255307777713450285,
                }),
            },
            Handle {
                site: 45,
                target: 45,
                container: 45,
                action: Generic(GenericAction {
                    value: I32(757935405),
                    bool: true,
                    key: 757935405,
                    pos: 2559517409927834925,
                    length: 3255307777747922309,
                    prop: 3255263599679974701,
                }),
            },
            Handle {
                site: 115,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-1),
                    bool: false,
                    key: 704642816,
                    pos: 2656912673916387165,
                    length: 3255307777954121093,
                    prop: 3307302277364711213,
                }),
            },
            Handle {
                site: 45,
                target: 45,
                container: 45,
                action: Generic(GenericAction {
                    value: I32(757935405),
                    bool: false,
                    key: 0,
                    pos: 6148914326292922597,
                    length: 72057594037927936,
                    prop: 18446181123739353088,
                }),
            },
            Handle {
                site: 255,
                target: 133,
                container: 133,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 2880154539,
                    pos: 17574988476103502763,
                    length: 3255307691477244185,
                    prop: 3255307777713450285,
                }),
            },
            Handle {
                site: 45,
                target: 45,
                container: 45,
                action: Generic(GenericAction {
                    value: I32(757935405),
                    bool: true,
                    key: 757935405,
                    pos: 3255307777713450285,
                    length: 3255307777713450285,
                    prop: 3255307780632685869,
                }),
            },
            Handle {
                site: 45,
                target: 45,
                container: 45,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 757935405,
                    pos: 3255307777713450285,
                    length: 3255307777713450285,
                    prop: 949464768499756507,
                }),
            },
            Handle {
                site: 45,
                target: 45,
                container: 45,
                action: Generic(GenericAction {
                    value: I32(757935405),
                    bool: true,
                    key: 757935405,
                    pos: 3255307777713450285,
                    length: 3255307777710620717,
                    prop: 3255307777713450285,
                }),
            },
            Sync { from: 35, to: 133 },
            Handle {
                site: 45,
                target: 45,
                container: 45,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 1929380141,
                    pos: 18446585211651489792,
                    length: 6713178190254243839,
                    prop: 9594038572176901375,
                }),
            },
            Handle {
                site: 45,
                target: 45,
                container: 45,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 757935405,
                    pos: 3255307777713450285,
                    length: 16501189034685508909,
                    prop: 4268049625796902912,
                }),
            },
            Sync { from: 171, to: 171 },
            Sync { from: 187, to: 25 },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: I32(253303065),
                    bool: true,
                    key: 421075225,
                    pos: 1804000721327167783,
                    length: 1808504320951916825,
                    prop: 1808504320951916825,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 58,
                action: Generic(GenericAction {
                    value: I32(421075225),
                    bool: true,
                    key: 223,
                    pos: 18446490146653077568,
                    length: 2559517409933624831,
                    prop: 3255307777724218171,
                }),
            },
            Checkout {
                site: 4,
                to: 3959422847,
            },
            Handle {
                site: 0,
                target: 118,
                container: 11,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 4284295679,
                    pos: 9621136720216981508,
                    length: 18387402525678320517,
                    prop: 3255308571086552873,
                }),
            },
            Handle {
                site: 45,
                target: 45,
                container: 45,
                action: Generic(GenericAction {
                    value: I32(757935405),
                    bool: true,
                    key: 757935405,
                    pos: 3255307777713450285,
                    length: 3255307777713450285,
                    prop: 3255307777713450285,
                }),
            },
            Handle {
                site: 45,
                target: 45,
                container: 45,
                action: Generic(GenericAction {
                    value: I32(-617796307),
                    bool: true,
                    key: 757935405,
                    pos: 3255307777713450285,
                    length: 3255307777722690861,
                    prop: 3255307777713450285,
                }),
            },
            Handle {
                site: 45,
                target: 45,
                container: 45,
                action: Generic(GenericAction {
                    value: I32(757935405),
                    bool: true,
                    key: 757935405,
                    pos: 2559517409927834925,
                    length: 3255307777747922309,
                    prop: 3255263599679974701,
                }),
            },
            Handle {
                site: 115,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-1),
                    bool: false,
                    key: 436207360,
                    pos: 1808504320951261465,
                    length: 16916094191212839,
                    prop: 1808476725365964800,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 133,
                action: Generic(GenericAction {
                    value: I32(-1532713820),
                    bool: false,
                    key: 2762253476,
                    pos: 11863788345444574372,
                    length: 18446744073693799588,
                    prop: 14377117046310043647,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 129,
                target: 133,
                container: 199,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 421075225,
                    pos: 1808504320951916858,
                    length: 18014398524102937,
                    prop: 14395694632364015616,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 4,
                target: 33,
                container: 65,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 3318153216,
                    pos: 14339461213547661511,
                    length: 14395693707582439552,
                    prop: 18446744073709551501,
                }),
            },
            Sync { from: 133, to: 199 },
            Handle {
                site: 199,
                target: 199,
                container: 199,
                action: Generic(GenericAction {
                    value: Container(Tree),
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
                    value: Container(Tree),
                    bool: true,
                    key: 1070057415,
                    pos: 14395705430045360250,
                    length: 14395694108859942855,
                    prop: 14395693700603168645,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 2239837695,
                    pos: 14395694392076126151,
                    length: 14395694394777216967,
                    prop: 14395621827009807669,
                }),
            },
            SyncAll,
            Handle {
                site: 133,
                target: 35,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(757935405),
                    bool: true,
                    key: 75181359,
                    pos: 8286624611106291583,
                    length: 18446585211651489792,
                    prop: 6713178190254243839,
                }),
            },
            Checkout {
                site: 223,
                to: 2240120100,
            },
            SyncAll,
            Sync { from: 255, to: 255 },
            SyncAll,
            Handle {
                site: 129,
                target: 133,
                container: 199,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 3351717831,
                    pos: 14395693707582490496,
                    length: 18446744073709551501,
                    prop: 14395621523933102079,
                }),
            },
            Handle {
                site: 45,
                target: 45,
                container: 199,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 2234894279,
                    pos: 4595861603807119303,
                    length: 71610056835194,
                    prop: 14395524409068552192,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 122,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-16777216),
                    bool: true,
                    key: 45,
                    pos: 0,
                    length: 0,
                    prop: 0,
                }),
            },
        ],
    )
}

#[test]
fn delta_err_3() {
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
                site: 0,
                target: 0,
                container: 11,
                action: Generic(GenericAction {
                    value: I32(-65475),
                    bool: true,
                    key: 67108863,
                    pos: 72057594037871521,
                    length: 217020518514230020,
                    prop: 280883327347844581,
                }),
            },
            Sync { from: 163, to: 215 },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(257),
                    bool: false,
                    key: 0,
                    pos: 0,
                    length: 7936,
                    prop: 18446743017316026880,
                }),
            },
            Handle {
                site: 133,
                target: 0,
                container: 199,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: false,
                    key: 2717908991,
                    pos: 2155061665,
                    length: 18446463698227810304,
                    prop: 9476562641788076031,
                }),
            },
            Sync { from: 255, to: 255 },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 791643507,
                    pos: 18446744073709551568,
                    length: 2965947086361162589,
                    prop: 18446744070104864675,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 199,
                action: Generic(GenericAction {
                    value: I32(-1547197533),
                    bool: true,
                    key: 4294912471,
                    pos: 0,
                    length: 0,
                    prop: 0,
                }),
            },
        ],
    )
}

#[test]
fn delta_err_4() {
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
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(-404281047),
                    bool: true,
                    key: 3890735079,
                    pos: 15287796486090778599,
                    length: 2965947086361143593,
                    prop: 8874669797471234345,
                }),
            },
            Handle {
                site: 59,
                target: 41,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4294967295,
                    pos: 16044073672507391,
                    length: 3026418949592973311,
                    prop: 2965947086361143593,
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
                    key: 3890735079,
                    pos: 16710579925595711463,
                    length: 16710579925595711463,
                    prop: 2965947089230047719,
                }),
            },
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 690563583,
                    pos: 2965947086361143593,
                    length: 16710579925595711273,
                    prop: 16710579925595711463,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(-54999),
                    bool: true,
                    key: 4294967295,
                    pos: 3026418949592973311,
                    length: 2971008166175692667,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690579070,
                    pos: 707340585,
                    length: 2965947086361198340,
                    prop: 16710370199142476073,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 2950134569,
                    pos: 18446743151283810209,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 690563369,
                    pos: 2965947086361143593,
                    length: 16710579925595711273,
                    prop: 16674551128576747495,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(707340585),
                    bool: false,
                    key: 67108864,
                    pos: 2965947086361143807,
                    length: 16710579106351753513,
                    prop: 16710579925595711463,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 690563369,
                    pos: 2966183304576892927,
                    length: 2965947086361143593,
                    prop: 16710579106351753513,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 212,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690563369,
                    pos: 18375812379578477097,
                    length: 2965947086361143593,
                    prop: 16710579922395539753,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 212,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 4280887593,
                    pos: 18446744073709551615,
                    length: 15527050319778283519,
                    prop: 18446507932719751599,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(694583015),
                    bool: true,
                    key: 707340585,
                    pos: 2966182222245134336,
                    length: 2965947086361143593,
                    prop: 16710579925595662633,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 3615172905,
                    pos: 18446507932719751599,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 690563369,
                    pos: 2965947086361143593,
                    length: 16710579925595662633,
                    prop: 7487207888740935655,
                }),
            },
            SyncAll,
            Handle {
                site: 212,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690563369,
                    pos: 3026141872662773802,
                    length: 2965947086361143593,
                    prop: 16710579925583210793,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(2066295081),
                    bool: true,
                    key: 607101359,
                    pos: 18446744073709496635,
                    length: 16710579925595711463,
                    prop: 2966135102861993959,
                }),
            },
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(10793),
                    bool: false,
                    key: 704578560,
                    pos: 2965947086361143593,
                    length: 16710579805324126505,
                    prop: 16710579925595711463,
                }),
            },
            SyncAll,
            Handle {
                site: 41,
                target: 212,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690563369,
                    pos: 16710579922395539753,
                    length: 16710579925595711463,
                    prop: 16710579925595678695,
                }),
            },
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(707340585),
                    bool: false,
                    key: 67108864,
                    pos: 2965947086361143807,
                    length: 16710579106351753513,
                    prop: 16710579925595711463,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 695937321,
                    pos: 4261583518885706537,
                    length: 18446744073709551401,
                    prop: 3314649325744685055,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(2129078569),
                    bool: false,
                    key: 690563369,
                    pos: 18375812379578477097,
                    length: 2965947086361143593,
                    prop: 16710579922395539753,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 212,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690563369,
                    pos: 2971008166175692667,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 0,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 690563369,
                    pos: 3026418949592973097,
                    length: 217020518514230057,
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
                    key: 50529027,
                    pos: 217020518514230019,
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
                    key: 50529027,
                    pos: 217020518514230019,
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
                    key: 690563331,
                    pos: 2965947086361143593,
                    length: 16710579925595662633,
                    prop: 7487207888740935655,
                }),
            },
            SyncAll,
            Handle {
                site: 212,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690563369,
                    pos: 3026141872662773802,
                    length: 2965947086361143593,
                    prop: 16710579925583210793,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(2066295081),
                    bool: true,
                    key: 607101359,
                    pos: 18446744073709496635,
                    length: 18446744073709551615,
                    prop: 18446744073709551405,
                }),
            },
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(-1),
                    bool: true,
                    key: 690563583,
                    pos: 2965947086361143593,
                    length: 16710579922395539753,
                    prop: 16710579925595711463,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: false,
                    key: 687931392,
                    pos: 4261583518885706537,
                    length: 18446744073709551401,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(-1582311557),
                    bool: true,
                    key: 4280892196,
                    pos: 16710580029079158783,
                    length: 16710579925595711463,
                    prop: 2965947086372350249,
                }),
            },
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(67108864),
                    bool: true,
                    key: 690563369,
                    pos: 2965947086361143593,
                    length: 16710579925595704295,
                    prop: 16710579925595711463,
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
                    key: 3615172905,
                    pos: 18446507932719751599,
                    length: 18446744073709551615,
                    prop: 18446743171766419455,
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
                    key: 690579070,
                    pos: 707340585,
                    length: 2965947086361198340,
                    prop: 16710370199142476073,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 10596311,
                    pos: 2965948004860690432,
                    length: 18446507855493802281,
                    prop: 2965947089965547519,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690563369,
                    pos: 16710579922395539753,
                    length: 16710579925595711463,
                    prop: 16710579925595678695,
                }),
            },
            SyncAll,
            Handle {
                site: 49,
                target: 54,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(690563381),
                    bool: true,
                    key: 690563369,
                    pos: 288230376154474793,
                    length: 2965947086361143807,
                    prop: 16710579106351753513,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 212,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690563369,
                    pos: 4261583518885706537,
                    length: 18446744073709551401,
                    prop: 3314649325744685055,
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
                    value: I32(690563369),
                    bool: true,
                    key: 4294967295,
                    pos: 2965947086361198591,
                    length: 2965947086361143593,
                    prop: 16710579925595711273,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: false,
                    key: 687931392,
                    pos: 4261583518885706537,
                    length: 18446744073709551401,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
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
fn delta_err_5() {
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
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(-1960236759),
                    bool: true,
                    key: 2341178251,
                    pos: 10055284024492657547,
                    length: 10055284174816512907,
                    prop: 10055284024492657547,
                }),
            },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 185, to: 185 },
            Sync { from: 139, to: 139 },
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(704578611),
                    bool: true,
                    key: 992553257,
                    pos: 18446507855497821737,
                    length: 2965907327158321151,
                    prop: 3026141872662773802,
                }),
            },
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 690563369,
                    pos: 2965947086361143593,
                    length: 16710579925595711463,
                    prop: 111264917479,
                }),
            },
            Checkout {
                site: 49,
                to: 2139042100,
            },
            Checkout {
                site: 127,
                to: 3890734975,
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(65536),
                    bool: false,
                    key: 0,
                    pos: 2965947086358523649,
                    length: 9187202097275697961,
                    prop: 16681191730380242815,
                }),
            },
            SyncAll,
            Handle {
                site: 41,
                target: 212,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690563369,
                    pos: 2971008166413171497,
                    length: 41,
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

#[test]
fn test_movable_list_20() {
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
                site: 0,
                target: 64,
                container: 36,
                action: Generic(GenericAction {
                    value: I32(993737531),
                    bool: true,
                    key: 2248146944,
                    pos: 4268102928402430779,
                    length: 18446468096294861627,
                    prop: 4268007270886932479,
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
                    length: 13021085206519788724,
                    prop: 13021234409388684468,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(59),
                    bool: false,
                    key: 83886080,
                    pos: 14033993531614298111,
                    length: 3369469612973540034,
                    prop: 18446743810689123010,
                }),
            },
            Handle {
                site: 59,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: I32(100663296),
                    bool: false,
                    key: 0,
                    pos: 144114088564228096,
                    length: 3835724633218956091,
                    prop: 4268070197442653696,
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
                    pos: 18446744073693102080,
                    length: 18389322329063621119,
                    prop: 0,
                }),
            },
        ],
    )
}

#[test]
fn test_movable_list_21() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Map, FuzzTarget::MovableList],
        &mut [
            Handle {
                site: 0,
                target: 35,
                container: 10,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4278386687,
                    pos: 2676586395023179775,
                    length: 2676586395008836901,
                    prop: 2676586395008836901,
                }),
            },
            Handle {
                site: 37,
                target: 37,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(-256),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 9895591935999,
                    prop: 3746994885677285376,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 37,
                target: 37,
                container: 37,
                action: Generic(GenericAction {
                    value: I32(623191333),
                    bool: true,
                    key: 3052521929,
                    pos: 18446744073709551615,
                    length: 18446744073709551615,
                    prop: 18446742987082838527,
                }),
            },
            Handle {
                site: 255,
                target: 253,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(265),
                    bool: false,
                    key: 4294967295,
                    pos: 16493559407536242687,
                    length: 3904675852509832420,
                    prop: 3834029159525384194,
                }),
            },
            Handle {
                site: 53,
                target: 53,
                container: 53,
                action: Generic(GenericAction {
                    value: I32(892679477),
                    bool: true,
                    key: 805255221,
                    pos: 72110370596060693,
                    length: 2108141021440,
                    prop: 2676586395008837078,
                }),
            },
            Handle {
                site: 37,
                target: 37,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(-256),
                    bool: true,
                    key: 4294967295,
                    pos: 18086737578496602879,
                    length: 18385141895277182975,
                    prop: 18446742974197924095,
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
                    value: Container(MovableList),
                    bool: true,
                    key: 1627389741,
                    pos: 67553161186901867,
                    length: 2676586395008836901,
                    prop: 13110481856001025317,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 3,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 151597311,
                    pos: 18446742974197924105,
                    length: 16493559523500359679,
                    prop: 3472838262201312484,
                }),
            },
        ],
    )
}

#[test]
fn test_movable_list_22() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Map, FuzzTarget::MovableList],
        &mut [
            Handle {
                site: 0,
                target: 35,
                container: 10,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4278386687,
                    pos: 2676586395023179775,
                    length: 2676586395008836901,
                    prop: 2676586395008836901,
                }),
            },
            Handle {
                site: 37,
                target: 37,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(-256),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 9895604649983,
                    prop: 3746994885677285376,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 37,
                target: 37,
                container: 37,
                action: Generic(GenericAction {
                    value: I32(623183141),
                    bool: true,
                    key: 3052521929,
                    pos: 18446744073709551615,
                    length: 18446744073709551615,
                    prop: 18446742987082838527,
                }),
            },
            Handle {
                site: 255,
                target: 253,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(265),
                    bool: false,
                    key: 4294967295,
                    pos: 16493559407536242687,
                    length: 3616728050846459108,
                    prop: 3834029159525384194,
                }),
            },
            Handle {
                site: 53,
                target: 53,
                container: 53,
                action: Generic(GenericAction {
                    value: I32(892679477),
                    bool: true,
                    key: 805255221,
                    pos: 72110370596060693,
                    length: 2108141021440,
                    prop: 2676586395008837078,
                }),
            },
            Handle {
                site: 37,
                target: 37,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(-256),
                    bool: true,
                    key: 4294967295,
                    pos: 18086737578496602879,
                    length: 2533271535615999,
                    prop: 18446742974197923840,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 37,
                container: 37,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: true,
                    key: 623191333,
                    pos: 2676586395008836901,
                    length: 2676586395008836901,
                    prop: 13088935740243780901,
                }),
            },
            Sync { from: 255, to: 123 },
            Handle {
                site: 56,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Map),
                    bool: true,
                    key: 4294967295,
                    pos: 1155454779397242677,
                    length: 3026417850081345536,
                    prop: 18446744073709496617,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
        ],
    )
}

#[test]
fn test_movable_list_23() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Map, FuzzTarget::MovableList],
        &mut [
            Handle {
                site: 0,
                target: 35,
                container: 10,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4278386687,
                    pos: 2676586395023175679,
                    length: 2676586395008836901,
                    prop: 2676586395008836901,
                }),
            },
            Handle {
                site: 37,
                target: 37,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(-256),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 9895591935999,
                    prop: 3746994885677285376,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 37,
                target: 37,
                container: 37,
                action: Generic(GenericAction {
                    value: I32(623191333),
                    bool: true,
                    key: 3052521929,
                    pos: 18390785747968851967,
                    length: 18446744073696116735,
                    prop: 18446742987082838527,
                }),
            },
            Handle {
                site: 255,
                target: 253,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(265),
                    bool: false,
                    key: 4294967295,
                    pos: 16493559407536242687,
                    length: 3904675852509832420,
                    prop: 3834029159525384194,
                }),
            },
            Handle {
                site: 53,
                target: 53,
                container: 53,
                action: Generic(GenericAction {
                    value: I32(892679477),
                    bool: true,
                    key: 792199476,
                    pos: 72110370596060693,
                    length: 2108141021440,
                    prop: 2676586395008837078,
                }),
            },
            Handle {
                site: 37,
                target: 37,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(-256),
                    bool: true,
                    key: 4294967295,
                    pos: 12587190423081956095,
                    length: 12587190073825341102,
                    prop: 71968184339443374,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 8,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 151597311,
                    pos: 18446742974197924105,
                    length: 16493559523500359679,
                    prop: 18374686483511829732,
                }),
            },
            Checkout {
                site: 0,
                to: 2667577554,
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 9437183,
                    pos: 13672292010998890496,
                    length: 13672292666392100285,
                    prop: 13672292666396491197,
                }),
            },
            Handle {
                site: 37,
                target: 37,
                container: 37,
                action: Generic(GenericAction {
                    value: I32(623191333),
                    bool: true,
                    key: 3048580413,
                    pos: 3401761431743735221,
                    length: 13552791426844167701,
                    prop: 65395,
                }),
            },
        ],
    )
}

#[test]
fn test_movable_list_24() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Map, FuzzTarget::MovableList],
        &mut [
            Handle {
                site: 96,
                target: 255,
                container: 36,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 721449843,
                    pos: 4292910109560504340,
                    length: 18446513810110911379,
                    prop: 3746994894253522943,
                }),
            },
            Sync { from: 93, to: 167 },
            Checkout {
                site: 123,
                to: 4286282619,
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 253,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: false,
                    key: 1553399936,
                    pos: 18390449078367420160,
                    length: 36201467589165055,
                    prop: 18374967954631580824,
                }),
            },
            SyncAll,
            Handle {
                site: 43,
                target: 43,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 2071690107,
                    pos: 18446744071486274427,
                    length: 17216960130726232104,
                    prop: 7430094960254740462,
                }),
            },
            SyncAll,
            Handle {
                site: 43,
                target: 43,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 2726001531,
                    pos: 18446744071486274427,
                    length: 17216960130726232104,
                    prop: 7430094960254740462,
                }),
            },
            Checkout {
                site: 0,
                to: 6094847,
            },
            Sync { from: 139, to: 139 },
            Sync { from: 139, to: 139 },
            Handle {
                site: 139,
                target: 43,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 2071690107,
                    pos: 18446744071486274427,
                    length: 17178700329823240232,
                    prop: 7430094960254715374,
                }),
            },
            Checkout {
                site: 0,
                to: 6094847,
            },
            Sync { from: 139, to: 139 },
            Sync { from: 55, to: 55 },
            Handle {
                site: 55,
                target: 55,
                container: 55,
                action: Generic(GenericAction {
                    value: I32(-1820655817),
                    bool: true,
                    key: 2483000211,
                    pos: 18387915803576970899,
                    length: 1297036689260150784,
                    prop: 8897841259086306727,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 253,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: false,
                    key: 1553399936,
                    pos: 18390449078367420160,
                    length: 36201467589165055,
                    prop: 18374967954631580824,
                }),
            },
            SyncAll,
            Handle {
                site: 43,
                target: 43,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 2071690107,
                    pos: 18446744071486274427,
                    length: 17216960130726232104,
                    prop: 7430094960254740462,
                }),
            },
            SyncAll,
            Handle {
                site: 43,
                target: 43,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 2726001531,
                    pos: 18446744071486274427,
                    length: 17216960130726232104,
                    prop: 7430094960254740462,
                }),
            },
            Checkout {
                site: 0,
                to: 6094847,
            },
            Sync { from: 139, to: 139 },
            Sync { from: 139, to: 139 },
            Handle {
                site: 139,
                target: 43,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 2071690107,
                    pos: 18446744071486274427,
                    length: 17178700329823240232,
                    prop: 7430094960254715374,
                }),
            },
            Checkout {
                site: 0,
                to: 6094847,
            },
            Sync { from: 139, to: 139 },
            Sync { from: 55, to: 55 },
            Sync { from: 155, to: 155 },
            Sync { from: 155, to: 155 },
            Sync { from: 155, to: 55 },
            Handle {
                site: 55,
                target: 55,
                container: 55,
                action: Generic(GenericAction {
                    value: I32(151587327),
                    bool: true,
                    key: 4294904073,
                    pos: 143835908481745196,
                    length: 11691225419712927791,
                    prop: 10638384274776138274,
                }),
            },
            Sync { from: 147, to: 20 },
            Handle {
                site: 34,
                target: 34,
                container: 34,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: false,
                    key: 3749681,
                    pos: 18389705808507043840,
                    length: 245750159833,
                    prop: 15697858579274924032,
                }),
            },
            SyncAll,
        ],
    )
}

#[test]
fn test_unknown() {
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
                site: 34,
                target: 115,
                container: 4,
                action: Generic(GenericAction {
                    value: I32(62063364),
                    bool: false,
                    key: 771987715,
                    pos: 217020518514230019,
                    length: 217234923281646339,
                    prop: 6234107865851074949,
                }),
            },
            Handle {
                site: 3,
                target: 3,
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
fn test_movable_list_25() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Map, FuzzTarget::MovableList],
        &mut [
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4294967295,
                    pos: 8791026472627208191,
                    length: 18446744073692774400,
                    prop: 18157383382357508095,
                }),
            },
            SyncAll,
            Checkout {
                site: 126,
                to: 285217792,
            },
            Handle {
                site: 0,
                target: 238,
                container: 96,
                action: Generic(GenericAction {
                    value: I32(-256),
                    bool: true,
                    key: 2634743807,
                    pos: 34084,
                    length: 18446656643131973376,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Handle {
                site: 43,
                target: 93,
                container: 246,
                action: Generic(GenericAction {
                    value: Container(Map),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 2133084,
                    prop: 18446744073709486080,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 47,
                action: Generic(GenericAction {
                    value: I32(402652951),
                    bool: true,
                    key: 387389207,
                    pos: 1663900941173922844,
                    length: 1663823975275763479,
                    prop: 2738087021684922135,
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
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 4294967295,
                    pos: 4899916394579099647,
                    length: 1519401924284121087,
                    prop: 18446744069768287677,
                }),
            },
            SyncAll,
            Handle {
                site: 3,
                target: 3,
                container: 3,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: false,
                    key: 50398464,
                    pos: 216172795049214723,
                    length: 18427888561740251907,
                    prop: 18442733055291424767,
                }),
            },
            Handle {
                site: 23,
                target: 23,
                container: 23,
                action: Generic(GenericAction {
                    value: I32(1124602995),
                    bool: false,
                    key: 4280877827,
                    pos: 18446744073709551615,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 162,
                target: 3,
                container: 3,
                action: Generic(GenericAction {
                    value: I32(-34201),
                    bool: true,
                    key: 84215045,
                    pos: 11791549724362736901,
                    length: 1513209197156392029,
                    prop: 11817445422215987071,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 16,
                target: 51,
                container: 52,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 4294967056,
                    pos: 13258596207762079743,
                    length: 216172782113733631,
                    prop: 18428093872899751680,
                }),
            },
            SyncAll,
            Handle {
                site: 9,
                target: 23,
                container: 23,
                action: Generic(GenericAction {
                    value: I32(1340183),
                    bool: true,
                    key: 2053581568,
                    pos: 18446509877732900863,
                    length: 4836866087641415679,
                    prop: 1152922604118419456,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 247,
                target: 255,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: false,
                    key: 521,
                    pos: 0,
                    length: 0,
                    prop: 18446499166067621888,
                }),
            },
            Handle {
                site: 47,
                target: 47,
                container: 47,
                action: Generic(GenericAction {
                    value: I32(3631),
                    bool: false,
                    key: 0,
                    pos: 15646670743035716644,
                    length: 606348324,
                    prop: 3832285106451644416,
                }),
            },
            Checkout {
                site: 255,
                to: 1946103807,
            },
            Handle {
                site: 247,
                target: 255,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-193),
                    bool: true,
                    key: 767,
                    pos: 18446742974197924864,
                    length: 1620130366750719,
                    prop: 3605423971803043822,
                }),
            },
            Handle {
                site: 9,
                target: 9,
                container: 1,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 56823,
                    pos: 18446744073709551399,
                    length: 71777218572845055,
                    prop: 6947083900195729408,
                }),
            },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 2442236305,
                    pos: 10489325061521117585,
                    length: 10489325061521117585,
                    prop: 10489325061521117585,
                }),
            },
            Sync { from: 145, to: 145 },
            Sync { from: 145, to: 145 },
            Sync { from: 145, to: 145 },
            Sync { from: 145, to: 145 },
            Sync { from: 145, to: 145 },
            Sync { from: 145, to: 145 },
            Sync { from: 145, to: 145 },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(454761243),
                    bool: true,
                    key: 454761243,
                    pos: 1953184666628070171,
                    length: 143834851265485595,
                    prop: 18446744073692890520,
                }),
            },
            Handle {
                site: 162,
                target: 3,
                container: 3,
                action: Generic(GenericAction {
                    value: I32(-34201),
                    bool: true,
                    key: 84215045,
                    pos: 11791549724362736901,
                    length: 1513209197156392029,
                    prop: 11817445422215987071,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 16,
                target: 51,
                container: 52,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 4294967056,
                    pos: 13258596207762079743,
                    length: 216172782113733631,
                    prop: 18428093872899751680,
                }),
            },
            SyncAll,
            Handle {
                site: 9,
                target: 23,
                container: 23,
                action: Generic(GenericAction {
                    value: I32(387389207),
                    bool: true,
                    key: 4390932,
                    pos: 8346507721238538027,
                    length: 0,
                    prop: 0,
                }),
            },
        ],
    )
}

#[test]
fn test_tree_delete_nested() {
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
                site: 49,
                target: 53,
                container: 57,
                action: Generic(GenericAction {
                    value: Container(Map),
                    bool: true,
                    key: 960051513,
                    pos: 4123389851770370361,
                    length: 4123390026907529521,
                    prop: 4123389851767159097,
                }),
            },
            Handle {
                site: 8,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(968243513),
                    bool: true,
                    key: 960051513,
                    pos: 18446529914715322681,
                    length: 18432897712502263502,
                    prop: 4123389851781761081,
                }),
            },
            Handle {
                site: 57,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(-1),
                    bool: true,
                    key: 4294967295,
                    pos: 4179058995052806143,
                    length: 4123389851770370361,
                    prop: 4123389851770370361,
                }),
            },
            Handle {
                site: 57,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: Container(Map),
                    bool: true,
                    key: 3472883517,
                    pos: 18446744072884244174,
                    length: 281474976710655,
                    prop: 18446496683593302016,
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
                    length: 2531906049332683555,
                    prop: 2531906049332683555,
                }),
            },
            SyncAll,
            Handle {
                site: 61,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(960051513),
                    bool: true,
                    key: 3684665,
                    pos: 4109597577911548258,
                    length: 13058531512473434622,
                    prop: 4123390386434848181,
                }),
            },
            Handle {
                site: 61,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(960051513),
                    bool: true,
                    key: 3684665,
                    pos: 592567745925167458,
                    length: 3904956473749929529,
                    prop: 592567743911911936,
                }),
            },
            Handle {
                site: 57,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(960075065),
                    bool: true,
                    key: 960051513,
                    pos: 4123389851770370486,
                    length: 14915921129292511545,
                    prop: 18446689986361085646,
                }),
            },
            Handle {
                site: 61,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(960051513),
                    bool: true,
                    key: 3684665,
                    pos: 4109597577911548258,
                    length: 15825516631767550,
                    prop: 16647618940511009122,
                }),
            },
            Handle {
                site: 57,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(-1254540999),
                    bool: true,
                    key: 3048584629,
                    pos: 4123389851770370361,
                    length: 4123389851770370361,
                    prop: 13093570749027399993,
                }),
            },
            Handle {
                site: 57,
                target: 57,
                container: 57,
                action: Generic(GenericAction {
                    value: Container(Map),
                    bool: true,
                    key: 960051513,
                    pos: 4123389850948286777,
                    length: 4123389851770370356,
                    prop: 18446743862421043513,
                }),
            },
        ],
    )
}

#[test]
fn test_text() {
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
                site: 255,
                target: 15,
                container: 57,
                action: Generic(GenericAction {
                    value: I32(757935405),
                    bool: true,
                    key: 805292845,
                    pos: 33041,
                    length: 0,
                    prop: 1238489669910396928,
                }),
            },
            Handle {
                site: 45,
                target: 45,
                container: 45,
                action: Generic(GenericAction {
                    value: I32(757935405),
                    bool: true,
                    key: 805292845,
                    pos: 3255307780432560401,
                    length: 18446743168229387565,
                    prop: 3255307777713581326,
                }),
            },
            Handle {
                site: 45,
                target: 45,
                container: 45,
                action: Generic(GenericAction {
                    value: I32(757935405),
                    bool: true,
                    key: 4291505453,
                    pos: 18388247646700638511,
                    length: 18446744073709507839,
                    prop: 5570344,
                }),
            },
        ],
    )
}

#[test]
fn test_text_del_2() {
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
                site: 42,
                target: 45,
                container: 253,
                action: Generic(GenericAction {
                    value: I32(33554179),
                    bool: false,
                    key: 15616,
                    pos: 1339615555336169111,
                    length: 10909519737336631312,
                    prop: 10923365712002484737,
                }),
            },
            Sync { from: 151, to: 151 },
            Handle {
                site: 191,
                target: 0,
                container: 2,
                action: Generic(GenericAction {
                    value: I32(-1088190176),
                    bool: false,
                    key: 1898119453,
                    pos: 114672903794094449,
                    length: 2593958586217895690,
                    prop: 16131857654658175249,
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
                    pos: 1229782938247310353,
                    length: 1229782938247303441,
                    prop: 1229782938247303441,
                }),
            },
            Handle {
                site: 21,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(33554176),
                    bool: false,
                    key: 0,
                    pos: 1536,
                    length: 0,
                    prop: 1229782938247303424,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 0,
                    pos: 1229782864946528256,
                    length: 12080808152476417826,
                    prop: 10923366098543524643,
                }),
            },
            Handle {
                site: 35,
                target: 38,
                container: 35,
                action: Generic(GenericAction {
                    value: I32(587333693),
                    bool: false,
                    key: 2543294434,
                    pos: 4263285121861231497,
                    length: 59,
                    prop: 1518013315106421504,
                }),
            },
            Sync { from: 167, to: 35 },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 49,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: false,
                    key: 0,
                    pos: 3602878813425696768,
                    length: 18446743185255244799,
                    prop: 8152360975528560127,
                }),
            },
            SyncAll,
            Handle {
                site: 80,
                target: 0,
                container: 61,
                action: Generic(GenericAction {
                    value: I32(1161822054),
                    bool: true,
                    key: 269488146,
                    pos: 10883513199263901286,
                    length: 10923366098549554583,
                    prop: 2748041329745827735,
                }),
            },
            Handle {
                site: 61,
                target: 0,
                container: 2,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 160113,
                    pos: 4438182027029079654,
                    length: 1229782938564925335,
                    prop: 1229785140740218641,
                }),
            },
            Handle {
                site: 63,
                target: 17,
                container: 17,
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
                site: 17,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 1536,
                    pos: 0,
                    length: 1229782864946528256,
                    prop: 1229782938247303441,
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
                    key: 17,
                    pos: 1229764173248856064,
                    length: 12080626724467843601,
                    prop: 10923366097000014759,
                }),
            },
            Handle {
                site: 1,
                target: 35,
                container: 38,
                action: Generic(GenericAction {
                    value: I32(33570048),
                    bool: true,
                    key: 2543313634,
                    pos: 3043090847611718039,
                    length: 15163,
                    prop: 1229783119343321088,
                }),
            },
            Sync { from: 167, to: 167 },
            Handle {
                site: 1,
                target: 191,
                container: 35,
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
fn unknown_container() {
    let doc = loro_without_counter::LoroDoc::new();
    let list = doc.get_list("list");
    doc.subscribe(
        &list.id(),
        Arc::new(|e| {
            assert_eq!(e.events.len(), 2);
        }),
    );

    let doc2 = LoroDoc::new();
    let list2 = doc2.get_list("list");
    let counter = list2.insert_container(0, LoroCounter::new()).unwrap();
    counter.increment(2.).unwrap();

    doc.import(&doc2.export_snapshot()).unwrap();
}

#[test]
fn undo_tree() {
    test_multi_sites(
        5,
        vec![FuzzTarget::Tree],
        &mut [
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: ActionWrapper::Action(fuzz::actions::ActionInner::Tree(TreeAction {
                    target: (0, 0),
                    action: TreeActionInner::Create { index: 0 },
                })),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: ActionWrapper::Action(fuzz::actions::ActionInner::Tree(TreeAction {
                    target: (0, 1),
                    action: TreeActionInner::Create { index: 1 },
                })),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: ActionWrapper::Action(fuzz::actions::ActionInner::Tree(TreeAction {
                    target: (0, 0),
                    action: TreeActionInner::Move {
                        parent: (0, 1),
                        index: 0,
                    },
                })),
            },
            Handle {
                site: 1,
                target: 0,
                container: 0,
                action: ActionWrapper::Action(fuzz::actions::ActionInner::Tree(TreeAction {
                    target: (0, 1),
                    action: TreeActionInner::Move {
                        parent: (0, 0),
                        index: 0,
                    },
                })),
            },
            SyncAllUndo { site: 0, op_len: 1 },
        ],
    )
}

#[test]
fn unknown_test() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 33,
                target: 33,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 555819305,
                    pos: 1378419387125539105,
                    length: 7143833951692390400,
                    prop: 18158512740587612165,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 33,
                target: 33,
                container: 33,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 2165440511,
                    pos: 18446713166840815823,
                    length: 18446744073695002623,
                    prop: 9300496180473495551,
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
            Handle {
                site: 33,
                target: 88,
                container: 33,
                action: Generic(GenericAction {
                    value: I32(555819297),
                    bool: true,
                    key: 603922721,
                    pos: 18384011962329072995,
                    length: 2387470752459784191,
                    prop: 18157383382342902049,
                }),
            },
            SyncAll,
            Handle {
                site: 129,
                target: 207,
                container: 96,
                action: Generic(GenericAction {
                    value: Container(Unknown(227)),
                    bool: true,
                    key: 570425343,
                    pos: 18446744073709551615,
                    length: 1252228849668718591,
                    prop: 18446744073707709423,
                }),
            },
            Undo {
                site: 33,
                op_len: 557850913,
            },
            Handle {
                site: 33,
                target: 33,
                container: 33,
                action: Generic(GenericAction {
                    value: I32(553656616),
                    bool: true,
                    key: 4294967295,
                    pos: 2387225703671136255,
                    length: 18157383382357244923,
                    prop: 9300496180473495547,
                }),
            },
            SyncAll,
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(-81714911),
                    bool: true,
                    key: 4227595259,
                    pos: 9222240656569400315,
                    length: 18157383382357508095,
                    prop: 18157383382357244923,
                }),
            },
            Handle {
                site: 33,
                target: 33,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4213252369,
                    pos: 18157383382357244923,
                    length: 1229782942188567547,
                    prop: 1229764366808715537,
                }),
            },
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: Container(Map),
                    bool: true,
                    key: 2846468521,
                    pos: 1808504320692955561,
                    length: 1808504320951916825,
                    prop: 1805689571184810265,
                }),
            },
            Checkout {
                site: 59,
                to: 421533977,
            },
            SyncAllUndo {
                site: 9,
                op_len: 421075225,
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: I32(420419865),
                    bool: true,
                    key: 993737531,
                    pos: 1808504320951916859,
                    length: 1808504320951916825,
                    prop: 1808504320531568921,
                }),
            },
            Handle {
                site: 25,
                target: 126,
                container: 25,
                action: Generic(GenericAction {
                    value: I32(421075225),
                    bool: true,
                    key: 186194201,
                    pos: 3537886577862187264,
                    length: 4268070197444284185,
                    prop: 1808504320951916859,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: Container(Unknown(251)),
                    bool: true,
                    key: 4227595259,
                    pos: 2387466337166031867,
                    length: 18446744069971452193,
                    prop: 1297036692682702847,
                }),
            },
            SyncAll,
            SyncAllUndo {
                site: 207,
                op_len: 3824095584,
            },
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(-81714911),
                    bool: true,
                    key: 4227595259,
                    pos: 18157110720653425659,
                    length: 18157383382424616831,
                    prop: 18157383382357244923,
                }),
            },
            Handle {
                site: 33,
                target: 33,
                container: 33,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 555815423,
                    pos: 18157351368541544737,
                    length: 18158513695410093051,
                    prop: 18157383382357244923,
                }),
            },
            SyncAll,
            Handle {
                site: 33,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 291557213,
                    pos: 18384256508149097455,
                    length: 2449927290233290751,
                    prop: 2387226647993516031,
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
            Handle {
                site: 33,
                target: 33,
                container: 33,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 301989887,
                    pos: 18157383382357188897,
                    length: 9222241721654180859,
                    prop: 2387226643631439871,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 33,
                target: 33,
                container: 33,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 3823363055,
                    pos: 18157383399589937123,
                    length: 18157383382357244923,
                    prop: 2387225703656586235,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 33,
                target: 223,
                container: 47,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 4227596287,
                    pos: 18157383382357244923,
                    length: 2387225707328306171,
                    prop: 18446744073709494561,
                }),
            },
            SyncAll,
            Handle {
                site: 239,
                target: 227,
                container: 227,
                action: Generic(GenericAction {
                    value: I32(-1),
                    bool: true,
                    key: 4294967295,
                    pos: 16776961,
                    length: 0,
                    prop: 0,
                }),
            },
        ],
    )
}

#[test]
fn unknown_test_1() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 171,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 555876351,
                    pos: 1873497441278722081,
                    length: 18446680051950142745,
                    prop: 14251013405919093505,
                }),
            },
            Sync { from: 197, to: 197 },
            SyncAll,
            Handle {
                site: 25,
                target: 25,
                container: 197,
                action: Generic(GenericAction {
                    value: Container(Counter),
                    bool: true,
                    key: 589496831,
                    pos: 14251014049101066245,
                    length: 14251014049101104581,
                    prop: 1808504321235615743,
                }),
            },
            Handle {
                site: 0,
                target: 255,
                container: 35,
                action: Generic(GenericAction {
                    value: Container(Unknown(0)),
                    bool: false,
                    key: 93,
                    pos: 18388478750059857152,
                    length: 3540954258656722723,
                    prop: 14251014049104920573,
                }),
            },
            Sync { from: 197, to: 197 },
            Handle {
                site: 25,
                target: 231,
                container: 230,
                action: Generic(GenericAction {
                    value: Container(Counter),
                    bool: true,
                    key: 4294952389,
                    pos: 14251013405919093505,
                    length: 18446680051950142917,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 33,
                target: 33,
                container: 33,
                action: Generic(GenericAction {
                    value: I32(555819297),
                    bool: true,
                    key: 555819297,
                    pos: 2387225703656530209,
                    length: 18097157671754073472,
                    prop: 18446743085883913466,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Sync { from: 197, to: 255 },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: I32(23325),
                    bool: false,
                    key: 587595548,
                    pos: 71875463170644529,
                    length: 102254863057045,
                    prop: 3602878813425696768,
                }),
            },
            Checkout {
                site: 35,
                to: 4281410559,
            },
            Handle {
                site: 35,
                target: 49,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 427,
                    pos: 3535659958131563009,
                    length: 9259681213650633203,
                    prop: 2387225703656530209,
                }),
            },
            Handle {
                site: 33,
                target: 33,
                container: 33,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: false,
                    key: 4278283372,
                    pos: 2387226295835536299,
                    length: 388193571007627297,
                    prop: 2387225703656544040,
                }),
            },
            Handle {
                site: 33,
                target: 33,
                container: 51,
                action: Generic(GenericAction {
                    value: I32(892744248),
                    bool: true,
                    key: 16055035,
                    pos: 2387242053152086529,
                    length: 2387225703656530209,
                    prop: 2387225703656530209,
                }),
            },
            SyncAllUndo {
                site: 5,
                op_len: 84215075,
            },
        ],
    )
}

#[test]
fn unknown_test_2() {
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
            // SyncAll,
            // Handle {
            //     site: 27,
            //     target: 27,
            //     container: 27,
            //     action: Generic(GenericAction {
            //         value: I32(454761243),
            //         bool: true,
            //         key: 587202560,
            //         pos: 2499870512,
            //         length: 1955426454418212347,
            //         prop: 1953184666628070171,
            //     }),
            // },
            // Handle {
            //     site: 27,
            //     target: 27,
            //     container: 27,
            //     action: Generic(GenericAction {
            //         value: I32(-404232217),
            //         bool: true,
            //         key: 18606055,
            //         pos: 1953184666626555904,
            //         length: 1953184666628070171,
            //         prop: 1953184666627808063,
            //     }),
            // },
            // Handle {
            //     site: 27,
            //     target: 27,
            //     container: 27,
            //     action: Generic(GenericAction {
            //         value: I32(454761243),
            //         bool: false,
            //         key: 807600128,
            //         pos: 29802787832063,
            //         length: 163831513883392,
            //         prop: 2527082340907941888,
            //     }),
            // },
            // Handle {
            //     site: 27,
            //     target: 27,
            //     container: 27,
            //     action: Generic(GenericAction {
            //         value: I32(-1920103141),
            //         bool: true,
            //         key: 2374864269,
            //         pos: 10199964370168810893,
            //         length: 10199964370168810893,
            //         prop: 10199964370168810893,
            //     }),
            // },
            SyncAllUndo {
                site: 141,
                op_len: 2374864269,
            },
        ],
    )
}

#[test]
fn unknown_test_3() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 0,
                target: 255,
                container: 203,
                action: Generic(GenericAction {
                    value: Container(Map),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 1,
                    prop: 506381209866536711,
                }),
            },
            Handle {
                site: 7,
                target: 7,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(117901063),
                    bool: true,
                    key: 2332033031,
                    pos: 506381364485359499,
                    length: 17268955636171736839,
                    prop: 506381949047603367,
                }),
            },
            SyncAll,
            Handle {
                site: 3,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(117901063),
                    bool: true,
                    key: 4219143943,
                    pos: 539160350683790203,
                    length: 18446744073693102591,
                    prop: 506381188273799167,
                }),
            },
            Handle {
                site: 39,
                target: 39,
                container: 39,
                action: Generic(GenericAction {
                    value: I32(656877351),
                    bool: true,
                    key: 79017397,
                    pos: 18446744073558687743,
                    length: 18446471394817474559,
                    prop: 2821266740699193087,
                }),
            },
            Handle {
                site: 0,
                target: 49,
                container: 54,
                action: Generic(GenericAction {
                    value: I32(939524701),
                    bool: false,
                    key: 3419143984,
                    pos: 18388267869089287115,
                    length: 16492674416639,
                    prop: 18446744073709551360,
                }),
            },
            Handle {
                site: 7,
                target: 7,
                container: 7,
                action: Generic(GenericAction {
                    value: I32(-75823353),
                    bool: true,
                    key: 2071690107,
                    pos: 18446468104864627579,
                    length: 47269824462061567,
                    prop: 18446743004391874983,
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
                    pos: 13053445094070757159,
                    length: 2199006786997,
                    prop: 576459871835127808,
                }),
            },
            SyncAll,
            Handle {
                site: 39,
                target: 39,
                container: 39,
                action: Generic(GenericAction {
                    value: I32(656877351),
                    bool: true,
                    key: 656877351,
                    pos: 18385707052877424423,
                    length: 506381209866536193,
                    prop: 543681012144998151,
                }),
            },
            Handle {
                site: 7,
                target: 7,
                container: 7,
                action: Generic(GenericAction {
                    value: I32(-1953824768),
                    bool: true,
                    key: 4281009927,
                    pos: 18446744073709551615,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Handle {
                site: 7,
                target: 7,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 117901235,
                    pos: 18376096049401366279,
                    length: 576460752303423487,
                    prop: 506381209866536706,
                }),
            },
            Undo {
                site: 0,
                op_len: 125533051,
            },
        ],
    )
}

#[test]
fn b_delete_parent_tree_undo() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 4,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(454761243),
                    bool: true,
                    key: 454761243,
                    pos: 1953184666628070171,
                    length: 1953184666628070171,
                    prop: 1953184666560961307,
                }),
            },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(454761243),
                    bool: true,
                    key: 5,
                    pos: 163831513883392,
                    length: 2527082340907941888,
                    prop: 1953184666628070171,
                }),
            },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(-404232421),
                    bool: true,
                    key: 3890735079,
                    pos: 16710579925595711463,
                    length: 16710579925595711463,
                    prop: 16710579925595711463,
                }),
            },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(741092379),
                    bool: false,
                    key: 875901996,
                    pos: 56299273401911,
                    length: 2527082340907941888,
                    prop: 1953184666643014442,
                }),
            },
            SyncAll,
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331155),
                    bool: true,
                    key: 286331153,
                    pos: 18446481363731288337,
                    length: 18446744073709551615,
                    prop: 18446743047498699025,
                }),
            },
            SyncAll,
            Handle {
                site: 17,
                target: 17,
                container: 1,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 286331153,
                    pos: 18446744073709551615,
                    length: 1227230898458525695,
                    prop: 1229787336293814545,
                }),
            },
            SyncAllUndo {
                site: 17,
                op_len: 291557249,
            },
        ],
    )
}

#[test]
fn unknown_undo_err() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 48,
                target: 255,
                container: 203,
                action: Generic(GenericAction {
                    value: Container(Map),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 1,
                    prop: 506381209866536711,
                }),
            },
            Handle {
                site: 7,
                target: 7,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(117901063),
                    bool: true,
                    key: 2332033031,
                    pos: 506381364485359499,
                    length: 17268955636171736839,
                    prop: 506381949047603367,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(117901063),
                    bool: true,
                    key: 4219143943,
                    pos: 539160350683790203,
                    length: 18446744073693102591,
                    prop: 506381188273799167,
                }),
            },
            Handle {
                site: 39,
                target: 39,
                container: 39,
                action: Generic(GenericAction {
                    value: I32(656877351),
                    bool: true,
                    key: 79017397,
                    pos: 18446744073558687743,
                    length: 18446471394817474559,
                    prop: 2821266740699193087,
                }),
            },
            Handle {
                site: 48,
                target: 49,
                container: 54,
                action: Generic(GenericAction {
                    value: I32(939524701),
                    bool: false,
                    key: 3419143984,
                    pos: 18388267869089287115,
                    length: 16492674416639,
                    prop: 18446744073709551360,
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
                    pos: 13053445094070757159,
                    length: 2199017665973,
                    prop: 576459871835127808,
                }),
            },
            SyncAll,
            Handle {
                site: 39,
                target: 39,
                container: 39,
                action: Generic(GenericAction {
                    value: I32(656877351),
                    bool: true,
                    key: 656877351,
                    pos: 18385905728160868135,
                    length: 9826112617566373121,
                    prop: 506381211308916736,
                }),
            },
            Handle {
                site: 0,
                target: 139,
                container: 139,
                action: Generic(GenericAction {
                    value: I32(-1),
                    bool: true,
                    key: 2071690107,
                    pos: 506378989234256763,
                    length: 1978051601041159,
                    prop: 18386797630442932992,
                }),
            },
            SyncAll,
            Undo {
                site: 123,
                op_len: 4278680443,
            },
            SyncAll,
            Handle {
                site: 39,
                target: 39,
                container: 39,
                action: Generic(GenericAction {
                    value: I32(35733558),
                    bool: false,
                    key: 939524701,
                    pos: 14685055086132932610,
                    length: 18446744070224098211,
                    prop: 18446462598733823999,
                }),
            },
            SyncAll,
            Undo {
                site: 123,
                op_len: 4278680443,
            },
        ],
    )
}

#[test]
fn unknown_undo_err_1() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(1600085855),
                    bool: true,
                    key: 6250319,
                    pos: 18374686479540944896,
                    length: 8589934592,
                    prop: 18446508778221142016,
                }),
            },
            SyncAll,
            Handle {
                site: 2,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: false,
                    key: 0,
                    pos: 1374464301510623240,
                    length: 2676586395008832275,
                    prop: 18446744073587533093,
                }),
            },
            Handle {
                site: 37,
                target: 37,
                container: 37,
                action: Generic(GenericAction {
                    value: I32(-476635),
                    bool: true,
                    key: 4294967295,
                    pos: 18446742974197926143,
                    length: 1157443315285966847,
                    prop: 10923210422339659366,
                }),
            },
            Handle {
                site: 37,
                target: 37,
                container: 37,
                action: Generic(GenericAction {
                    value: I32(-122018523),
                    bool: true,
                    key: 4294967295,
                    pos: 18446462598733430783,
                    length: 4400175265561444351,
                    prop: 18417301791145594896,
                }),
            },
            // SyncAll,
            Handle {
                site: 37,
                target: 37,
                container: 37,
                action: Generic(GenericAction {
                    value: I32(623191333),
                    bool: true,
                    key: 623192869,
                    pos: 2676586395008836901,
                    length: 10634005407190033701,
                    prop: 10634005407197270931,
                }),
            },
            Handle {
                site: 37,
                target: 37,
                container: 37,
                action: Generic(GenericAction {
                    value: I32(-8472027),
                    bool: true,
                    key: 4294967295,
                    pos: 18446742974197926143,
                    length: 1157443315285966847,
                    prop: 10883513199263901286,
                }),
            },
            // SyncAllUndo {
            //     site: 151,
            //     op_len: 2475923351,
            // },
            // SyncAll,
            // Handle {
            //     site: 37,
            //     target: 37,
            //     container: 37,
            //     action: Generic(GenericAction {
            //         value: I32(623191333),
            //         bool: true,
            //         key: 623192869,
            //         pos: 2676304920032126245,
            //         length: 10634005407190034213,
            //         prop: 4846998633281983379,
            //     }),
            // },
            // Checkout {
            //     site: 67,
            //     to: 1128481603,
            // },
            // SyncAllUndo {
            //     site: 146,
            //     op_len: 630428563,
            // },
            // Handle {
            //     site: 37,
            //     target: 37,
            //     container: 37,
            //     action: Generic(GenericAction {
            //         value: Container(Unknown(255)),
            //         bool: true,
            //         key: 2303,
            //         pos: 1676816805009555200,
            //         length: 7378697628035322064,
            //         prop: 10923267142493798807,
            //     }),
            // },
            // SyncAll,
            Handle {
                site: 37,
                target: 37,
                container: 37,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 589823,
                    pos: 4989988387126444032,
                    length: 4564488215,
                    prop: 18446629064793286160,
                }),
            },
            // SyncAll,
            // Handle {
            //     site: 37,
            //     target: 37,
            //     container: 37,
            //     action: Generic(GenericAction {
            //         value: I32(623191333),
            //         bool: true,
            //         key: 623191339,
            //         pos: 2676586395008836901,
            //         length: 10634005407197242661,
            //         prop: 10634005407197270931,
            //     }),
            // },
            Handle {
                site: 37,
                target: 37,
                container: 37,
                action: Generic(GenericAction {
                    value: I32(-1862),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744069414584328,
                    length: 7354395854818985279,
                    prop: 10923210423161742950,
                }),
            },
            // SyncAll,
            Handle {
                site: 37,
                target: 37,
                container: 33,
                action: Generic(GenericAction {
                    value: I32(-1862),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744069414584328,
                    length: 1157443315285963327,
                    prop: 10883513199263901286,
                }),
            },
            // SyncAllUndo {
            //     site: 151,
            //     op_len: 2475923351,
            // },
            // Handle {
            //     site: 37,
            //     target: 37,
            //     container: 37,
            //     action: Generic(GenericAction {
            //         value: I32(623191333),
            //         bool: true,
            //         key: 623191339,
            //         pos: 2676586395008836901,
            //         length: 10634005407197242661,
            //         prop: 10634005407197270931,
            //     }),
            // },
            Handle {
                site: 37,
                target: 37,
                container: 37,
                action: Generic(GenericAction {
                    value: I32(-33094),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744069414584328,
                    length: 7354395854818985279,
                    prop: 10923210423161742950,
                }),
            },
            // SyncAllUndo {
            //     site: 151,
            //     op_len: 2475922327,
            // },
            // SyncAll,
            // Handle {
            //     site: 37,
            //     target: 37,
            //     container: 37,
            //     action: Generic(GenericAction {
            //         value: I32(623191333),
            //         bool: true,
            //         key: 623191339,
            //         pos: 2676585295497209125,
            //         length: 10634005407197242663,
            //         prop: 4846792388952429459,
            //     }),
            // },
            Handle {
                site: 37,
                target: 37,
                container: 37,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 8,
                    pos: 14994529625533579263,
                    length: 10909519737336631312,
                    prop: 10923365712002484737,
                }),
            },
            SyncAllUndo {
                site: 147,
                op_len: 630428563,
            },
        ],
    )
}

#[test]
fn tree_delete_parent_and_delete_child() {
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
            Undo {
                site: 95,
                op_len: 1600085855,
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
                    length: 4107282860161957883,
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
            Undo {
                site: 209,
                op_len: 4291920003,
            },
        ],
    )
}

#[test]
fn undo_movable_list_0() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 255,
                target: 49,
                container: 25,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 421075225,
                    pos: 2096734697103628569,
                    length: 14952233337343252761,
                    prop: 1808504324185078624,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: I32(421085465),
                    bool: true,
                    key: 4284514577,
                    pos: 18375812379578413347,
                    length: 1,
                    prop: 4553463601266163552,
                }),
            },
            Handle {
                site: 255,
                target: 49,
                container: 25,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 421075225,
                    pos: 2096734697103628569,
                    length: 14952233337343252761,
                    prop: 1808504324185078624,
                }),
            },
            Handle {
                site: 5,
                target: 39,
                container: 49,
                action: Generic(GenericAction {
                    value: I32(-16646145),
                    bool: true,
                    key: 33554431,
                    pos: 4294967295,
                    length: 18446743369335635968,
                    prop: 288229276640083998,
                }),
            },
            Undo {
                site: 255,
                op_len: 654648099,
            },
        ],
    )
}

#[test]
fn undo_movable_list_1() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 4294908185,
                    pos: 18446744073709551615,
                    length: 18446744073709551615,
                    prop: 18446744073709551614,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 32,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-209),
                    bool: true,
                    key: 4294902219,
                    pos: 7036874838900735,
                    length: 1801721325924909056,
                    prop: 1808504320951916825,
                }),
            },
            Sync { from: 25, to: 25 },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: I32(85530905),
                    bool: false,
                    key: 0,
                    pos: 1808617462854123520,
                    length: 1808504381081458969,
                    prop: 1808504320951916825,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 185,
                action: Generic(GenericAction {
                    value: I32(421075225),
                    bool: true,
                    key: 1305,
                    pos: 7064362208460800,
                    length: 57362,
                    prop: 1808504320951910656,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 179,
                action: Generic(GenericAction {
                    value: I32(421075225),
                    bool: true,
                    key: 421075225,
                    pos: 21895911705,
                    length: 1808504320530841600,
                    prop: 1808504320951916825,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: I32(421075385),
                    bool: true,
                    key: 421075225,
                    pos: 2965947086361139481,
                    length: 2965947086361143617,
                    prop: 2965947047706437929,
                }),
            },
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(858335529),
                    bool: true,
                    key: 858993459,
                    pos: 10633899440147215155,
                    length: 10634005407197270931,
                    prop: 10634005407197270931,
                }),
            },
            SyncAllUndo {
                site: 147,
                op_len: 4287861651,
            },
            SyncAllUndo {
                site: 147,
                op_len: 2475922323,
            },
            SyncAllUndo {
                site: 147,
                op_len: 2475922323,
            },
            SyncAllUndo {
                site: 147,
                op_len: 2475922323,
            },
            SyncAllUndo {
                site: 147,
                op_len: 2475922323,
            },
            SyncAllUndo {
                site: 147,
                op_len: 2475922323,
            },
            Checkout {
                site: 51,
                to: 858993459,
            },
            Handle {
                site: 41,
                target: 32,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 1563240745,
                    pos: 2965947086361143593,
                    length: 2965946948922190121,
                    prop: 2965947086361143593,
                }),
            },
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690563369,
                    pos: 660104077147447337,
                    length: 2965947086361143593,
                    prop: 18446744070105147741,
                }),
            },
            SyncAll,
            Checkout {
                site: 255,
                to: 4294967295,
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 4294901760,
                    pos: 18446744073709551615,
                    length: 18446744073709551615,
                    prop: 2965948009088548863,
                }),
            },
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 689973545,
                    pos: 18386272210477721897,
                    length: 18446744073709551369,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(909127984),
                    bool: true,
                    key: 690563369,
                    pos: 2965947086361143593,
                    length: 2965947086360553769,
                    prop: 2965947086361143561,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294967109,
                    pos: 18446744073709551615,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690563369,
                    pos: 2965947086361143593,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 52,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(690555177),
                    bool: true,
                    key: 690563369,
                    pos: 2965947086361143593,
                    length: 2965947086361143593,
                    prop: 18446507855493802281,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690561833),
                    bool: true,
                    key: 690563369,
                    pos: 2965947086361143593,
                    length: 2965947086361143593,
                    prop: 660104193111566624,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 64,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690563369,
                    pos: 2965947086361143593,
                    length: 2963413811570747689,
                    prop: 2965946948922190121,
                }),
            },
            Handle {
                site: 93,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(858993459),
                    bool: true,
                    key: 858993459,
                    pos: 10634005407197270931,
                    length: 10634005407197270931,
                    prop: 10634005407197270931,
                }),
            },
            SyncAllUndo {
                site: 147,
                op_len: 2475922323,
            },
            SyncAllUndo {
                site: 147,
                op_len: 2475922323,
            },
            SyncAllUndo {
                site: 147,
                op_len: 697537427,
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
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
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
                    key: 690563369,
                    pos: 2965947086361143593,
                    length: 2965947047706437929,
                    prop: 3101290370670602537,
                }),
            },
            Checkout {
                site: 43,
                to: 723921707,
            },
            Checkout {
                site: 43,
                to: 724249387,
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690563369,
                    pos: 2965947086361143593,
                    length: 2963413811570747689,
                    prop: 18386272210477721897,
                }),
            },
            SyncAll,
            Checkout {
                site: 41,
                to: 690563369,
            },
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 589900073,
                    pos: 2965947086361143593,
                    length: 2965947047706437929,
                    prop: 18446507855491705129,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 64,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690563369,
                    pos: 2965947086361143593,
                    length: 2963413811570747689,
                    prop: 2965946948922190121,
                }),
            },
            Handle {
                site: 93,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(858993459),
                    bool: true,
                    key: 858993459,
                    pos: 10634005407197270931,
                    length: 10634005407197270931,
                    prop: 10634005407197270931,
                }),
            },
            SyncAllUndo {
                site: 147,
                op_len: 2475922323,
            },
            SyncAllUndo {
                site: 147,
                op_len: 2475922323,
            },
            SyncAllUndo {
                site: 147,
                op_len: 690590611,
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
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690563369,
                    pos: 18386272210477721897,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690561833),
                    bool: true,
                    key: 690563369,
                    pos: 2965947086361143593,
                    length: 2965947086361143593,
                    prop: 660104193111566624,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 64,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690563369,
                    pos: 2965947086361143593,
                    length: 2963413811570747689,
                    prop: 2965946948922190121,
                }),
            },
            Handle {
                site: 93,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(858993459),
                    bool: true,
                    key: 858993459,
                    pos: 10634005407197270931,
                    length: 10634005407197270931,
                    prop: 10634005407197270931,
                }),
            },
            SyncAllUndo {
                site: 147,
                op_len: 2475922323,
            },
            SyncAllUndo {
                site: 147,
                op_len: 2475922323,
            },
            SyncAllUndo {
                site: 147,
                op_len: 697537427,
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
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
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
                    key: 690563369,
                    pos: 2965947086361143593,
                    length: 2965947047706437929,
                    prop: 3101290370670602537,
                }),
            },
            Checkout {
                site: 43,
                to: 724249387,
            },
            Checkout {
                site: 43,
                to: 724249387,
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690563369,
                    pos: 2965947086361143593,
                    length: 2965937190756493609,
                    prop: 18446507855493802281,
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
                    key: 690618367,
                    pos: 2965947086361143593,
                    length: 2965947086361143587,
                    prop: 2965937190756493609,
                }),
            },
            Handle {
                site: 41,
                target: 41,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294967295,
                    pos: 4623507967449235455,
                    length: 2965947086361143593,
                    prop: 2965947086361143593,
                }),
            },
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690561065),
                    bool: true,
                    key: 688466217,
                    pos: 6712941976333396265,
                    length: 3689348814741252393,
                    prop: 10606877842382992179,
                }),
            },
            SyncAllUndo {
                site: 147,
                op_len: 2475922323,
            },
            SyncAllUndo {
                site: 147,
                op_len: 2475922323,
            },
            SyncAllUndo {
                site: 147,
                op_len: 2475922323,
            },
            SyncAllUndo {
                site: 147,
                op_len: 2475922323,
            },
            SyncAllUndo {
                site: 147,
                op_len: 2475922323,
            },
            Handle {
                site: 41,
                target: 9,
                container: 255,
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
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(690563369),
                    bool: true,
                    key: 690563369,
                    pos: 2965947086361143593,
                    length: 2963413811570747689,
                    prop: 18386272210477721897,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(808529977),
                    bool: true,
                    key: 690563369,
                    pos: 2963413811570747689,
                    length: 18386272210477721897,
                    prop: 18446744073709551369,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 41,
                target: 41,
                container: 41,
                action: Generic(GenericAction {
                    value: I32(809056049),
                    bool: true,
                    key: 691222064,
                    pos: 2965947086361143593,
                    length: 2965937190756493609,
                    prop: 2965947085824272681,
                }),
            },
            Undo {
                site: 41,
                op_len: 4294912297,
            },
            SyncAll,
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: I32(421075379),
                    bool: true,
                    key: 421101081,
                    pos: 1808604818470600729,
                    length: 1808504320951916825,
                    prop: 15384779033,
                }),
            },
        ],
    )
}

#[test]
fn unknown_fuzz_err() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 0),
                    action: TreeActionInner::Create { index: 0 },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 1),
                    action: TreeActionInner::Create { index: 0 },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 2),
                    action: TreeActionInner::Create { index: 2 },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 2),
                    action: TreeActionInner::Delete,
                })),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 1),
                    action: TreeActionInner::Move {
                        parent: (2, 0),
                        index: 0,
                    },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 0),
                    action: TreeActionInner::Meta {
                        meta: ("117901063".into(), I32(117901063)),
                    },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 0),
                    action: TreeActionInner::Meta {
                        meta: ("117901063".into(), I32(117908231)),
                    },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 0),
                    action: TreeActionInner::Delete,
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 7),
                    action: TreeActionInner::Create { index: 0 },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 7),
                    action: TreeActionInner::Delete,
                })),
            },
            SyncAll,
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 9),
                    action: TreeActionInner::Create { index: 0 },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 10),
                    action: TreeActionInner::Create { index: 0 },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 9),
                    action: TreeActionInner::Meta {
                        meta: ("117901063".into(), I32(119736071)),
                    },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 12),
                    action: TreeActionInner::Create { index: 0 },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 10),
                    action: TreeActionInner::Meta {
                        meta: ("117901091".into(), I32(117901063)),
                    },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 10),
                    action: TreeActionInner::Move {
                        parent: (2, 9),
                        index: 0,
                    },
                })),
            },
            SyncAll,
            Handle {
                site: 3,
                target: 3,
                container: 0,
                action: Action(ActionInner::Map(MapAction::Insert {
                    key: 17,
                    value: Container(Text),
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 12),
                    action: TreeActionInner::Meta {
                        meta: ("117901063".into(), I32(117908231)),
                    },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 12),
                    action: TreeActionInner::Meta {
                        meta: ("117917447".into(), I32(117901063)),
                    },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 12),
                    action: TreeActionInner::Meta {
                        meta: ("117901063".into(), I32(117901063)),
                    },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 18),
                    action: TreeActionInner::Create { index: 1 },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 12),
                    action: TreeActionInner::Meta {
                        meta: ("4227595259".into(), I32(555819297)),
                    },
                })),
            },
            Handle {
                site: 3,
                target: 0,
                container: 1,
                action: Action(ActionInner::Text(TextAction {
                    pos: 0,
                    len: 1,
                    action: TextActionInner::Insert,
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 12),
                    action: TreeActionInner::Meta {
                        meta: ("117901063".into(), I32(117901091)),
                    },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 12),
                    action: TreeActionInner::Meta {
                        meta: ("2231830279".into(), I32(117901063)),
                    },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 22),
                    action: TreeActionInner::Create { index: 0 },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 10),
                    action: TreeActionInner::Move {
                        parent: (2, 12),
                        index: 0,
                    },
                })),
            },
            Handle {
                site: 2,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 24),
                    action: TreeActionInner::Create { index: 2 },
                })),
            },
            Handle {
                site: 3,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 9),
                    action: TreeActionInner::Delete,
                })),
            },
            SyncAll,
            Handle {
                site: 3,
                target: 2,
                container: 0,
                action: Action(ActionInner::Tree(TreeAction {
                    target: (2, 10),
                    action: TreeActionInner::Meta {
                        meta: ("abc".into(), I32(123)),
                    },
                })),
            },
            SyncAllUndo { site: 3, op_len: 3 },
        ],
    )
}

#[test]
fn unknown_fuzz_err_1() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 21,
                target: 172,
                container: 237,
                action: Generic(GenericAction {
                    value: Container(Unknown(19)),
                    bool: false,
                    key: 4288555552,
                    pos: 11601534246259907033,
                    length: 11646767826930344353,
                    prop: 11601273739628618145,
                }),
            },
            SyncAllUndo {
                site: 161,
                op_len: 2711724449,
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 2711724288,
                    pos: 11646767826930344353,
                    length: 27553,
                    prop: 11646767826921955584,
                }),
            },
            SyncAllUndo {
                site: 161,
                op_len: 2711724449,
            },
            SyncAllUndo {
                site: 161,
                op_len: 2711724449,
            },
            SyncAllUndo {
                site: 161,
                op_len: 2711724449,
            },
            SyncAllUndo {
                site: 161,
                op_len: 2711724449,
            },
            SyncAllUndo {
                site: 161,
                op_len: 2711694497,
            },
            SyncAllUndo {
                site: 161,
                op_len: 2711724449,
            },
            SyncAllUndo {
                site: 161,
                op_len: 2711724449,
            },
            SyncAllUndo {
                site: 161,
                op_len: 2711724449,
            },
            Handle {
                site: 61,
                target: 1,
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
fn diff_calc_fuzz_err_1() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 143,
                target: 29,
                container: 98,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 3149642750,
                    pos: 18097429212317875131,
                    length: 64871186039035,
                    prop: 17565089386645696778,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Checkout {
                site: 55,
                to: 4294916923,
            },
            Handle {
                site: 251,
                target: 0,
                container: 239,
                action: Generic(GenericAction {
                    value: I32(657457152),
                    bool: true,
                    key: 656877351,
                    pos: 2821266740684990247,
                    length: 2826896240219203367,
                    prop: 17521015924422327227,
                }),
            },
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(426766319),
                    bool: true,
                    key: 3146720292,
                    pos: 18694838926267,
                    length: 10314409433236454331,
                    prop: 18391499916132989883,
                }),
            },
            SyncAll,
            Undo {
                site: 111,
                op_len: 1869573999,
            },
            Undo {
                site: 111,
                op_len: 1869573999,
            },
            Undo {
                site: 111,
                op_len: 1869573999,
            },
            Undo {
                site: 111,
                op_len: 4294966971,
            },
            Sync { from: 59, to: 255 },
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
fn diff_calc_fuzz_err_2() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 4,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-2105409536),
                    bool: false,
                    key: 4294967295,
                    pos: 137975431167,
                    length: 458752,
                    prop: 360287970189639680,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: false,
                    key: 0,
                    pos: 12948890936913428480,
                    length: 12948890938015724467,
                    prop: 12948890938015724467,
                }),
            },
            Sync { from: 179, to: 179 },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(825307392),
                    bool: true,
                    key: 825307441,
                    pos: 17361641481138352433,
                    length: 18302628885800892209,
                    prop: 65534,
                }),
            },
        ],
    )
}

#[test]
fn diff_calc_fuzz_err_3() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331137),
                    bool: true,
                    key: 286331153,
                    pos: 1229782938247303443,
                    length: 1229782938247303441,
                    prop: 1229782938247303441,
                }),
            },
            Handle {
                site: 243,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286332389),
                    bool: true,
                    key: 4294967057,
                    pos: 0,
                    length: 1229782938247303461,
                    prop: 1229782938247303441,
                }),
            },
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: false,
                    key: 0,
                    pos: 2676586395008827392,
                    length: 2676586395008836901,
                    prop: 17160162796632352037,
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
                    pos: 40841467208997,
                    length: 1290863008193515793,
                    prop: 2676586395008836881,
                }),
            },
            Handle {
                site: 17,
                target: 17,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Unknown(238)),
                    bool: false,
                    key: 286331153,
                    pos: 1229782938247303441,
                    length: 1230021532270531537,
                    prop: 2676586395008836901,
                }),
            },
            Handle {
                site: 17,
                target: 0,
                container: 37,
                action: Generic(GenericAction {
                    value: I32(286386705),
                    bool: true,
                    key: 286331153,
                    pos: 1229782938247303441,
                    length: 1229782941116208017,
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
                    pos: 1229775241665909130,
                    length: 1229782938247303441,
                    prop: 2676586395008836901,
                }),
            },
            Handle {
                site: 37,
                target: 37,
                container: 37,
                action: Generic(GenericAction {
                    value: I32(285212709),
                    bool: true,
                    key: 286331153,
                    pos: 1229782938247303658,
                    length: 1229782938247303441,
                    prop: 1229782938247303953,
                }),
            },
            Sync { from: 17, to: 17 },
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 286331153,
                    pos: 1229782938247303441,
                    length: 725379779989737745,
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
                    pos: 1277915159264825617,
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
                    key: 287904017,
                    pos: 18764998447377,
                    length: 2676586395008827392,
                    prop: 2676586395008836901,
                }),
            },
            Handle {
                site: 37,
                target: 37,
                container: 238,
                action: Generic(GenericAction {
                    value: I32(623191333),
                    bool: true,
                    key: 623191333,
                    pos: 2676586395008836901,
                    length: 17160162796632352037,
                    prop: 2676586395008836901,
                }),
            },
            Handle {
                site: 37,
                target: 37,
                container: 37,
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
                site: 37,
                target: 37,
                container: 37,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 286265617,
                    pos: 1229782938247303441,
                    length: 1229782938247303441,
                    prop: 1229782938247303441,
                }),
            },
            Handle {
                site: 138,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 286331153,
                    pos: 1229782938247303658,
                    length: 1229782938247303441,
                    prop: 1229782938247303953,
                }),
            },
            Sync { from: 17, to: 17 },
            Handle {
                site: 17,
                target: 17,
                container: 17,
                action: Generic(GenericAction {
                    value: I32(286331153),
                    bool: true,
                    key: 286331153,
                    pos: 1229782938247303441,
                    length: 1229782938280857873,
                    prop: 1277915159264825617,
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
                    length: 1236538337688359185,
                    prop: 18764998447377,
                }),
            },
        ],
    )
}

#[test]
fn fast_snapshot_0() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 254,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Map),
                    bool: true,
                    key: 48059,
                    pos: 13527611514411810816,
                    length: 11,
                    prop: 13527612320720337851,
                }),
            },
            Sync { from: 187, to: 187 },
            Sync { from: 187, to: 69 },
            Handle {
                site: 187,
                target: 187,
                container: 187,
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
fn fast_snapshot_1() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 39,
                target: 39,
                container: 39,
                action: Generic(GenericAction {
                    value: I32(654311424),
                    bool: true,
                    key: 656877351,
                    pos: 17578436819671263015,
                    length: 1710228712612688883,
                    prop: 10314409432589529071,
                }),
            },
            Sync { from: 187, to: 59 },
            Handle {
                site: 39,
                target: 39,
                container: 39,
                action: Generic(GenericAction {
                    value: I32(656877351),
                    bool: true,
                    key: 656877351,
                    pos: 2821279934824523559,
                    length: 11020573209995047,
                    prop: 2821266740028112896,
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
                    pos: 1953184666628076853,
                    length: 1953184666628070171,
                    prop: 1953184666628070171,
                }),
            },
        ],
    )
}

#[test]
fn fast_snapshot_2() {
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
                    length: 12080808863958804391,
                    prop: 12080808863958804391,
                }),
            },
            SyncAllUndo {
                site: 167,
                op_len: 2812782503,
            },
            Handle {
                site: 27,
                target: 27,
                container: 49,
                action: Generic(GenericAction {
                    value: I32(875640369),
                    bool: true,
                    key: 454761243,
                    pos: 1953184666628070298,
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
                    length: 1953184666628070235,
                    prop: 12041247832392499326,
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
                    value: I32(1499027801),
                    bool: true,
                    key: 1499027801,
                    pos: 6438275382588823897,
                    length: 6438275382588823897,
                    prop: 6438275382588823897,
                }),
            },
            SyncAllUndo {
                site: 37,
                op_len: 2812782503,
            },
        ],
    )
}

#[test]
fn fast_snapshot_3() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 4,
                target: 251,
                container: 251,
                action: Generic(GenericAction {
                    value: Container(Unknown(86)),
                    bool: true,
                    key: 555819297,
                    pos: 18446744073709551615,
                    length: 1252228849668718591,
                    prop: 2449958197287707631,
                }),
            },
            Handle {
                site: 33,
                target: 33,
                container: 33,
                action: Generic(GenericAction {
                    value: Container(Unknown(251)),
                    bool: true,
                    key: 4294967295,
                    pos: 9362721257822425599,
                    length: 18446713166840815823,
                    prop: 18446744073695002623,
                }),
            },
            Handle {
                site: 33,
                target: 33,
                container: 251,
                action: Generic(GenericAction {
                    value: Container(Unknown(251)),
                    bool: true,
                    key: 2147220227,
                    pos: 18157383382357508095,
                    length: 18157383382357244923,
                    prop: 18384011580076532219,
                }),
            },
            Handle {
                site: 223,
                target: 47,
                container: 222,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4227595259,
                    pos: 18157383382357244923,
                    length: 2387225703656586235,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 251,
                target: 251,
                container: 251,
                action: Generic(GenericAction {
                    value: Container(Unknown(3)),
                    bool: true,
                    key: 4294934523,
                    pos: 18157383382357508095,
                    length: 18157383382357244923,
                    prop: 18384011580076532219,
                }),
            },
            Handle {
                site: 33,
                target: 251,
                container: 251,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4286577659,
                    pos: 18157383382357245951,
                    length: 18157383382357244923,
                    prop: 18446499024906297633,
                }),
            },
            Handle {
                site: 223,
                target: 47,
                container: 222,
                action: Generic(GenericAction {
                    value: Container(Unknown(251)),
                    bool: true,
                    key: 4227595259,
                    pos: 18157383382357244923,
                    length: 2387225703656586235,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 33,
                target: 251,
                container: 251,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4286577659,
                    pos: 18157383382357245951,
                    length: 18157383382357244923,
                    prop: 18446499024906297633,
                }),
            },
            SyncAll,
            Handle {
                site: 223,
                target: 222,
                container: 221,
                action: Generic(GenericAction {
                    value: Container(Unknown(4)),
                    bool: true,
                    key: 4294967295,
                    pos: 2387209068692373503,
                    length: 2594064589257963313,
                    prop: 18100529071917779530,
                }),
            },
            Handle {
                site: 33,
                target: 251,
                container: 251,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4286577659,
                    pos: 18157383382357245951,
                    length: 18157383382357244923,
                    prop: 18446499024906297633,
                }),
            },
            Handle {
                site: 239,
                target: 227,
                container: 227,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 555819352,
                    pos: 2387225703656530209,
                    length: 9332677834833697,
                    prop: 18446744073709494561,
                }),
            },
            SyncAll,
            Handle {
                site: 33,
                target: 251,
                container: 251,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: false,
                    key: 4294907392,
                    pos: 1252228849668718591,
                    length: 18384256508149097455,
                    prop: 2387225707345346559,
                }),
            },
            SyncAll,
            Undo {
                site: 123,
                op_len: 2071690107,
            },
        ],
    )
}

#[test]
fn fast_snapshot_4() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 239,
                target: 59,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(-16711680),
                    bool: true,
                    key: 335544832,
                    pos: 18446744073709551615,
                    length: 7740398493674204159,
                    prop: 18400863652505714539,
                }),
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 21,
                target: 239,
                container: 59,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 71777218572845055,
                    prop: 18446744073709551615,
                }),
            },
            Undo {
                site: 107,
                op_len: 4294929259,
            },
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 21,
                target: 239,
                container: 59,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 281474976710655,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 239,
                target: 59,
                container: 59,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294967295,
                    pos: 18412122651574140927,
                    length: 18446744073709551615,
                    prop: 7740561859543039999,
                }),
            },
            Undo {
                site: 107,
                op_len: 4294967295,
            },
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            SyncAll,
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: I32(739842329),
                    bool: true,
                    key: 0,
                    pos: 9794485864112324608,
                    length: 1808504320952179997,
                    prop: 1808504324825808852,
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
fn fast_snapshot_5() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 25,
                target: 25,
                container: 193,
                action: Generic(GenericAction {
                    value: I32(1644825),
                    bool: false,
                    key: 393216,
                    pos: 27487790694400,
                    length: 1808504320951916800,
                    prop: 18446744073709551385,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 436207615,
                    pos: 1808504320951916825,
                    length: 16131858542891077913,
                    prop: 18410398479363858399,
                }),
            },
            SyncAll,
            Handle {
                site: 25,
                target: 34,
                container: 25,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 18446744073709551615,
                    prop: 18381750949675392991,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: I32(-538976367),
                    bool: true,
                    key: 685760479,
                    pos: 16131858456979185696,
                    length: 1811037595742312729,
                    prop: 1849036717598251289,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(421068800),
                    bool: true,
                    key: 432014617,
                    pos: 18446744073694419225,
                    length: 18446744073709551615,
                    prop: 16131858542891098079,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: Container(Unknown(223)),
                    bool: true,
                    key: 538978527,
                    pos: 1808722877991345952,
                    length: 1808504359606622489,
                    prop: 1808504939427207449,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(421075225),
                    bool: true,
                    key: 421075392,
                    pos: 18446744073709551513,
                    length: 16131858541569507327,
                    prop: 16131858542891098079,
                }),
            },
            Handle {
                site: 25,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 4279834905,
                    pos: 10455415605511192575,
                    length: 2945318833950285791,
                    prop: 16131858456979185696,
                }),
            },
            Handle {
                site: 223,
                target: 40,
                container: 32,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 421075225,
                    pos: 1808504320951916834,
                    length: 7064470003718569,
                    prop: 0,
                }),
            },
            Handle {
                site: 192,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 3750828543,
                    pos: 16131858542891098079,
                    length: 18446744073170575327,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: Container(Unknown(25)),
                    bool: true,
                    key: 421075225,
                    pos: 16131858204548733209,
                    length: 2314885568395206623,
                    prop: 1808505174690352095,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 25,
                    pos: 18446462598732840960,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: Container(Unknown(223)),
                    bool: true,
                    key: 538976296,
                    pos: 1808505174690352095,
                    length: 1808504321102911769,
                    prop: 1808504323367835929,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(435231001),
                    bool: true,
                    key: 421075392,
                    pos: 18446744073709551385,
                    length: 16131893865241116671,
                    prop: 8319119876378817395,
                }),
            },
            Undo {
                site: 115,
                op_len: 1936946035,
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: I32(-544138983),
                    bool: true,
                    key: 3755991007,
                    pos: 16126228219801118943,
                    length: 1808504320951967711,
                    prop: 1808504320951916834,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(421117957),
                    bool: true,
                    key: 4294967065,
                    pos: 18446744073709551615,
                    length: 16131858542891106303,
                    prop: 18446744073170575327,
                }),
            },
            Handle {
                site: 25,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 421075225,
                    pos: 16131858541569448217,
                    length: 16077885992209473503,
                    prop: 1808504324286832587,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: I32(421075225),
                    bool: true,
                    key: 0,
                    pos: 1808504213156659200,
                    length: 18417779746705245465,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 421075225,
                    pos: 16131858542885935385,
                    length: 14690495831856439263,
                    prop: 1808504320964943839,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: I32(421075225),
                    bool: false,
                    key: 0,
                    pos: 18446744069414584320,
                    length: 18446744073706012671,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(421075225),
                    bool: true,
                    key: 4294967065,
                    pos: 18446744073709551615,
                    length: 1808758200342674943,
                    prop: 18446490194318792985,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: Container(Unknown(25)),
                    bool: true,
                    key: 421075225,
                    pos: 16131858204548733209,
                    length: 2314885568395206623,
                    prop: 1808505174690352095,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 25,
                    pos: 1808476725365964800,
                    length: 1808505037875966233,
                    prop: 18446744073709551385,
                }),
            },
            Undo {
                site: 115,
                op_len: 1936946035,
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: Container(Unknown(223)),
                    bool: true,
                    key: 538976486,
                    pos: 1808505174690352095,
                    length: 1808504321102911769,
                    prop: 1808504323367835929,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(421117957),
                    bool: true,
                    key: 4294967065,
                    pos: 18446744073709551615,
                    length: 16131858542891106303,
                    prop: 18446744073170575327,
                }),
            },
            Handle {
                site: 25,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 421075225,
                    pos: 16131858541569448217,
                    length: 16077885992209473503,
                    prop: 1808504324286832587,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 421075225,
                    pos: 16131858542885935385,
                    length: 1808504320964943871,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: Container(Unknown(223)),
                    bool: true,
                    key: 538976296,
                    pos: 1808505174690352095,
                    length: 1808504321102911769,
                    prop: 1808504323367835929,
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
                    pos: 65535,
                    length: 1808476725365964800,
                    prop: 1801439851369273625,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294967295,
                    pos: 1808504320951975935,
                    length: 16131858204548733209,
                    prop: 16131858542891098079,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 34,
                action: Generic(GenericAction {
                    value: I32(-57089),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 18446744073709551615,
                    prop: 1808504324286840831,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: I32(-544138983),
                    bool: true,
                    key: 3755991007,
                    pos: 16126228219801118943,
                    length: 1808504320951967711,
                    prop: 1808504320951916834,
                }),
            },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: true,
                    key: 85530905,
                    pos: 18446743081993181632,
                    length: 18446744073709551615,
                    prop: 8319119878197870591,
                }),
            },
            Undo {
                site: 115,
                op_len: 1936946035,
            },
            SyncAll,
            Handle {
                site: 29,
                target: 29,
                container: 29,
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
fn gc_fuzz() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [Handle {
            site: 3,
            target: 251,
            container: 251,
            action: Generic(GenericAction {
                value: Container(Unknown(86)),
                bool: true,
                key: 555819297,
                pos: 18446744073709551615,
                length: 1252228849668718591,
                prop: 2449958197287707631,
            }),
        }],
    )
}

#[test]
fn gc_fuzz_1() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
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
fn gc_fuzz_2() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            SyncAll,
            Handle {
                site: 13,
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
fn gc_fuzz_3() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            Sync { from: 15, to: 231 },
            Sync { from: 231, to: 15 },
            Checkout { site: 0, to: 0 },
        ],
    )
}

#[test]
fn gc_fuzz_4() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            Checkout {
                site: 255,
                to: 4294967263,
            },
            Checkout {
                site: 65,
                to: 1094795585,
            },
            Checkout {
                site: 65,
                to: 1094795585,
            },
            Checkout {
                site: 65,
                to: 4294959103,
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
fn gc_fuzz_5() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            SyncAll,
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(0),
                    bool: false,
                    key: 4294967087,
                    pos: 144115188075855872,
                    length: 18446744073692774400,
                    prop: 13092193914696237055,
                }),
            },
            SyncAll,
        ],
    )
}

#[test]
fn gc_fuzz_6() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 39,
                target: 35,
                container: 39,
                action: Generic(GenericAction {
                    value: Container(Counter),
                    bool: true,
                    key: 0,
                    pos: 0,
                    length: 18446744073705569536,
                    prop: 4467570830337114111,
                }),
            },
            Handle {
                site: 39,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Counter),
                    bool: true,
                    key: 3250700737,
                    pos: 13961440319825297857,
                    length: 13961653383518601665,
                    prop: 4224835641023766945,
                }),
            },
            Sync { from: 29, to: 214 },
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
fn gc_fuzz_8() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 0,
                target: 0,
                container: 3,
                action: Generic(GenericAction {
                    value: Container(Counter),
                    bool: true,
                    key: 4279625703,
                    pos: 16992045493947160921,
                    length: 12731870089881583615,
                    prop: 72057593937267210,
                }),
            },
            Handle {
                site: 192,
                target: 89,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 3031741695,
                    pos: 5931894172722287193,
                    length: 18446691156916567897,
                    prop: 6438458614484300239,
                }),
            },
            Sync { from: 255, to: 255 },
            Handle {
                site: 99,
                target: 10,
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
fn gc_fuzz_7() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
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
fn gc_fuzz_9() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            Sync { from: 203, to: 203 },
            Sync { from: 211, to: 211 },
            Handle {
                site: 29,
                target: 151,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(1962876415),
                    bool: true,
                    key: 7453,
                    pos: 7049764864459276288,
                    length: 18446743039114868234,
                    prop: 2098070993983872557,
                }),
            },
            Handle {
                site: 126,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: I32(253581597),
                    bool: true,
                    key: 2653814783,
                    pos: 1737577045257614837,
                    length: 0,
                    prop: 15263776468834131248,
                }),
            },
            Sync { from: 29, to: 29 },
            SyncAllUndo {
                site: 61,
                op_len: 1962876415,
            },
            SyncAll,
            SyncAll,
            Handle {
                site: 13,
                target: 29,
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
fn gc_fuzz_10() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            SyncAll,
            Handle {
                site: 207,
                target: 97,
                container: 10,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 2307492175,
                    pos: 7047857333582417895,
                    length: 9910603677835267850,
                    prop: 9486702363886423311,
                }),
            },
            Checkout {
                site: 79,
                to: 267865931,
            },
            Sync { from: 207, to: 91 },
            Handle {
                site: 15,
                target: 13,
                container: 79,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 2206443401,
                    pos: 22250883543303043,
                    length: 9478258114588936448,
                    prop: 9910603588334613508,
                }),
            },
            SyncAllUndo {
                site: 139,
                op_len: 2341178251,
            },
            SyncAllUndo {
                site: 137,
                op_len: 2307492343,
            },
            Undo {
                site: 101,
                op_len: 13595768,
            },
        ],
    )
}

#[test]
fn gc_arb_test() {
    fn prop(u: &mut Unstructured<'_>, site_num: u8) -> arbitrary::Result<()> {
        let xs = u.arbitrary::<Vec<Action>>()?;
        if let Err(e) = std::panic::catch_unwind(|| {
            test_multi_sites_with_gc(site_num, vec![FuzzTarget::All], &mut xs.clone());
        }) {
            dbg!(xs);
            println!("{:?}", e);
            panic!()
        } else {
            Ok(())
        }
    }

    arbtest::builder().budget_ms(1000).run(|u| prop(u, 5))
}

#[test]
fn gc_fuzz_11() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            Sync { from: 193, to: 193 },
            Handle {
                site: 0,
                target: 0,
                container: 193,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: false,
                    key: 2711724449,
                    pos: 13961618035398779297,
                    length: 1008805371638175261,
                    prop: 18446744073697722215,
                }),
            },
            Undo {
                site: 27,
                op_len: 201271077,
            },
            Handle {
                site: 64,
                target: 0,
                container: 251,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 4278222847,
                    pos: 2669297569253097472,
                    length: 11357407135578062631,
                    prop: 14987979538418368581,
                }),
            },
            Handle {
                site: 0,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 2374864269,
                    pos: 13907115649332789645,
                    length: 13961486231981375937,
                    prop: 11646767826930540993,
                }),
            },
            Sync { from: 193, to: 65 },
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
fn gc_fuzz_12() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 45,
                target: 213,
                container: 97,
                action: Generic(GenericAction {
                    value: I32(1628047969),
                    bool: true,
                    key: 488447261,
                    pos: 2097865012304218831,
                    length: 2097865012304223517,
                    prop: 2097944487810827395,
                }),
            },
            Handle {
                site: 29,
                target: 29,
                container: 29,
                action: Generic(GenericAction {
                    value: I32(-1627389952),
                    bool: true,
                    key: 2678038431,
                    pos: 14816736806998876063,
                    length: 723498258358181791,
                    prop: 2097939697176227169,
                }),
            },
            Sync { from: 10, to: 29 },
            Handle {
                site: 25,
                target: 29,
                container: 29,
                action: Generic(GenericAction {
                    value: I32(1946452765),
                    bool: false,
                    key: 488465765,
                    pos: 2821267465459197725,
                    length: 2821266723505121319,
                    prop: 18446743142358525735,
                }),
            },
            Undo {
                site: 15,
                op_len: 3486502887,
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
fn gc_fuzz_13() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            Checkout {
                site: 81,
                to: 1364283729,
            },
            Sync { from: 81, to: 81 },
            Checkout {
                site: 81,
                to: 1364283729,
            },
            Undo {
                site: 101,
                op_len: 131329,
            },
            Handle {
                site: 81,
                target: 81,
                container: 81,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 2907787693,
                    pos: 5859553999884210605,
                    length: 5859553999884210513,
                    prop: 564055461160273,
                }),
            },
            Undo {
                site: 101,
                op_len: 1701144063,
            },
            Undo {
                site: 101,
                op_len: 1364328334,
            },
            Sync { from: 81, to: 193 },
            SyncAll,
            Handle {
                site: 1,
                target: 66,
                container: 221,
                action: Generic(GenericAction {
                    value: Container(Unknown(221)),
                    bool: true,
                    key: 150929629,
                    pos: 11646767925714553343,
                    length: 10458374703202721,
                    prop: 0,
                }),
            },
        ],
    )
}

#[test]
fn gc_fuzz_14() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 246,
                target: 89,
                container: 45,
                action: Generic(GenericAction {
                    value: Container(MovableList),
                    bool: true,
                    key: 488447261,
                    pos: 2098043135048944157,
                    length: 325135608980512029,
                    prop: 11502087481250378356,
                }),
            },
            Handle {
                site: 91,
                target: 37,
                container: 1,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4294967295,
                    pos: 228492260147199,
                    length: 2097838391875325903,
                    prop: 748474284404536324,
                }),
            },
            Handle {
                site: 231,
                target: 29,
                container: 29,
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
fn gc_fuzz_15() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 7,
                target: 7,
                container: 7,
                action: Generic(GenericAction {
                    value: Container(Unknown(186)),
                    bool: true,
                    key: 117901063,
                    pos: 2965947086361134855,
                    length: 2965947086361143593,
                    prop: 3388685387995481904,
                }),
            },
            Handle {
                site: 7,
                target: 7,
                container: 7,
                action: Generic(GenericAction {
                    value: I32(1745291055),
                    bool: true,
                    key: 117911303,
                    pos: 8144486177886908167,
                    length: 8216543772599977735,
                    prop: 2956339408958288498,
                }),
            },
            Handle {
                site: 7,
                target: 7,
                container: 7,
                action: Generic(GenericAction {
                    value: Container(Counter),
                    bool: true,
                    key: 3217014719,
                    pos: 13816973012072644543,
                    length: 13816973012072644543,
                    prop: 13816973012072644543,
                }),
            },
            Sync { from: 191, to: 191 },
            Handle {
                site: 41,
                target: 43,
                container: 7,
                action: Generic(GenericAction {
                    value: I32(-876939015),
                    bool: true,
                    key: 117901063,
                    pos: 2965947086361143559,
                    length: 10964339960146635049,
                    prop: 3388685387995481904,
                }),
            },
            Handle {
                site: 7,
                target: 7,
                container: 7,
                action: Generic(GenericAction {
                    value: I32(1745291055),
                    bool: true,
                    key: 117911303,
                    pos: 506381209866546951,
                    length: 8246661594435503367,
                    prop: 2965909556371288690,
                }),
            },
            SyncAll,
        ],
    )
}

#[test]
fn gc_fuzz_16() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            SyncAll,
            Handle {
                site: 41,
                target: 252,
                container: 255,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 2981212593,
                    pos: 7276179889,
                    length: 12804210589391912960,
                    prop: 4035225269105177009,
                }),
            },
            Sync { from: 177, to: 177 },
            Sync { from: 177, to: 177 },
            Sync { from: 177, to: 177 },
            Sync { from: 177, to: 177 },
            Sync { from: 177, to: 177 },
            Sync { from: 177, to: 177 },
            Handle {
                site: 0,
                target: 0,
                container: 0,
                action: Generic(GenericAction {
                    value: Container(Unknown(17)),
                    bool: false,
                    key: 3590258688,
                    pos: 18434875764103304661,
                    length: 18446744073709540821,
                    prop: 18446744073706788309,
                }),
            },
            SyncAll,
            SyncAll,
            SyncAllUndo {
                site: 151,
                op_len: 2543294359,
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
fn gc_fuzz_17() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 146,
                target: 29,
                container: 29,
                action: Generic(GenericAction {
                    value: I32(-256),
                    bool: true,
                    key: 4294967295,
                    pos: 18446744073709551615,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
                }),
            },
            SyncAll,
            Handle {
                site: 159,
                target: 159,
                container: 159,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 181345565,
                    pos: 2097865012304223517,
                    length: 325135608980512029,
                    prop: 12474158554978546292,
                }),
            },
            Handle {
                site: 29,
                target: 0,
                container: 159,
                action: Generic(GenericAction {
                    value: I32(9570),
                    bool: false,
                    key: 2678038431,
                    pos: 11502087481254191007,
                    length: 748113402599022495,
                    prop: 2116984339537158410,
                }),
            },
            Handle {
                site: 159,
                target: 159,
                container: 159,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 168452565,
                    pos: 2097939697176227169,
                    length: 18446743659775762187,
                    prop: 18446744073709551615,
                }),
            },
            Handle {
                site: 29,
                target: 29,
                container: 29,
                action: Generic(GenericAction {
                    value: I32(488447261),
                    bool: true,
                    key: 1701999620,
                    pos: 32985348447589,
                    length: 2097865012302774272,
                    prop: 11460961887305145630,
                }),
            },
            SyncAllUndo {
                site: 159,
                op_len: 2678000543,
            },
        ],
    )
}

#[test]
fn gc_fuzz_18() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            SyncAll,
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(286989083),
                    bool: true,
                    key: 4288224017,
                    pos: 1953436334768717823,
                    length: 1956843841325308699,
                    prop: 1953184666628070171,
                }),
            },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(-1994712289),
                    bool: true,
                    key: 461970203,
                    pos: 1953184666628070170,
                    length: 1953184722462645019,
                    prop: 9879520012645636895,
                }),
            },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(454764571),
                    bool: true,
                    key: 454761243,
                    pos: 1953185035995257627,
                    length: 8150137753889872667,
                    prop: 13093571283691877813,
                }),
            },
            Sync { from: 181, to: 181 },
            Handle {
                site: 255,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: Container(Unknown(232)),
                    bool: true,
                    key: 4294967295,
                    pos: 13093571280777456127,
                    length: 13093571283691877813,
                    prop: 13093401958901200309,
                }),
            },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(-400876773),
                    bool: false,
                    key: 3907578088,
                    pos: 1984146914073451291,
                    length: 1953184666628070170,
                    prop: 1953184722462645019,
                }),
            },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(454761243),
                    bool: true,
                    key: 3044088603,
                    pos: 576460752303423487,
                    length: 37167066886380315,
                    prop: 1808504322244529934,
                }),
            },
            SyncAllUndo {
                site: 137,
                op_len: 454761243,
            },
        ],
    )
}

#[test]
fn gc_fuzz_19() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 0,
                target: 236,
                container: 231,
                action: Generic(GenericAction {
                    value: Container(Unknown(255)),
                    bool: true,
                    key: 4194303999,
                    pos: 15552899962701356511,
                    length: 15553137160186484695,
                    prop: 9910603133095716107,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: I32(421075225),
                    bool: true,
                    key: 421075225,
                    pos: 1808504320951916825,
                    length: 1808504320951916825,
                    prop: 18446744073702480153,
                }),
            },
            SyncAll,
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(454761243),
                    bool: true,
                    key: 454761243,
                    pos: 1808504320951975707,
                    length: 1808504320951916825,
                    prop: 18381750949675342105,
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
                    pos: 1376723914332045083,
                    length: 1953224249046670107,
                    prop: 1953184666628070171,
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
                    pos: 1952340241697938203,
                    length: 7378697629477444379,
                    prop: 1953184666628089446,
                }),
            },
            Handle {
                site: 25,
                target: 137,
                container: 137,
                action: Generic(GenericAction {
                    value: Container(Counter),
                    bool: true,
                    key: 3351168967,
                    pos: 14395694394768869319,
                    length: 14395694394777257927,
                    prop: 14395625957724096967,
                }),
            },
            SyncAll,
            Handle {
                site: 11,
                target: 122,
                container: 10,
                action: Generic(GenericAction {
                    value: Container(Map),
                    bool: true,
                    key: 3621246935,
                    pos: 9874839321247995769,
                    length: 1808504803874670985,
                    prop: 1808504320951916825,
                }),
            },
            Handle {
                site: 25,
                target: 25,
                container: 25,
                action: Generic(GenericAction {
                    value: I32(421075225),
                    bool: true,
                    key: 421075225,
                    pos: 18446743610274158873,
                    length: 18446744073709551615,
                    prop: 18446744073709551615,
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
                    pos: 1808504320966990619,
                    length: 1808504320951916825,
                    prop: 1808504320951916825,
                }),
            },
            SyncAll,
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(454761243),
                    bool: true,
                    key: 454761243,
                    pos: 1953184668522060571,
                    length: 1963317765789653779,
                    prop: 1953184666628070171,
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
                    pos: 1953184666628070171,
                    length: 7378697627851496219,
                    prop: 1953184666633004646,
                }),
            },
            SyncAll,
            Handle {
                site: 11,
                target: 122,
                container: 10,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 421075225,
                    pos: 1808504320951916825,
                    length: 1808504320951916825,
                    prop: 1808504320951916825,
                }),
            },
            Handle {
                site: 25,
                target: 137,
                container: 137,
                action: Generic(GenericAction {
                    value: Container(Counter),
                    bool: true,
                    key: 3338895559,
                    pos: 14395693845021444030,
                    length: 7378697211608767259,
                    prop: 1953184667891295846,
                }),
            },
            SyncAll,
            Handle {
                site: 25,
                target: 13,
                container: 79,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: false,
                    key: 4160222989,
                    pos: 9910674047567919095,
                    length: 9910667733958756745,
                    prop: 15553137160186484565,
                }),
            },
            SyncAllUndo {
                site: 140,
                op_len: 4278189964,
            },
        ],
    )
}

#[test]
fn gc_fuzz_20() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            SyncAll,
            SyncAll,
            Handle {
                site: 27,
                target: 229,
                container: 228,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 4288224017,
                    pos: 2017611710352523263,
                    length: 2889933389121133339,
                    prop: 1953184666628070171,
                }),
            },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(454762267),
                    bool: true,
                    key: 2300255003,
                    pos: 1953184666628069915,
                    length: 1953198960279231259,
                    prop: 16637674123620458267,
                }),
            },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(455613211),
                    bool: true,
                    key: 454761243,
                    pos: 1953279224628058907,
                    length: 1953184666628070171,
                    prop: 13093570503287832689,
                }),
            },
            Sync { from: 181, to: 181 },
            Undo {
                site: 113,
                op_len: 3048545137,
            },
            Sync { from: 181, to: 181 },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(454761243),
                    bool: true,
                    key: 469048091,
                    pos: 1953305612907387675,
                    length: 1953184666611321115,
                    prop: 2889933389121133339,
                }),
            },
            Handle {
                site: 27,
                target: 119,
                container: 228,
                action: Generic(GenericAction {
                    value: I32(454761242),
                    bool: true,
                    key: 454761243,
                    pos: 1953184666846173979,
                    length: 8150137753889872667,
                    prop: 1953184666628070171,
                }),
            },
            Handle {
                site: 2,
                target: 0,
                container: 181,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 3048584629,
                    pos: 13093571281108186549,
                    length: 13093571283691877813,
                    prop: 8174439530702681525,
                }),
            },
            Sync { from: 181, to: 181 },
            Sync { from: 181, to: 181 },
            Handle {
                site: 181,
                target: 181,
                container: 181,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 1903277493,
                    pos: 31836475601132401,
                    length: 13093571283679969793,
                    prop: 13093571283691877813,
                }),
            },
            Handle {
                site: 181,
                target: 181,
                container: 181,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 1903277493,
                    pos: 13093570621121589617,
                    length: 13093571283691877813,
                    prop: 13050224137278436789,
                }),
            },
            Handle {
                site: 181,
                target: 181,
                container: 181,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 3941264106,
                    pos: 1982722105925435391,
                    length: 8150137753889872667,
                    prop: 1953184666628070171,
                }),
            },
            Sync { from: 181, to: 181 },
            Sync { from: 181, to: 181 },
            Sync { from: 181, to: 27 },
            Sync { from: 181, to: 181 },
            Sync { from: 181, to: 181 },
            Undo {
                site: 255,
                op_len: 4294967295,
            },
            Handle {
                site: 181,
                target: 181,
                container: 181,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 2307503541,
                    pos: 15553137160186484565,
                    length: 13093571283691831297,
                    prop: 1953185037443773877,
                }),
            },
            Handle {
                site: 2,
                target: 0,
                container: 181,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 3048584629,
                    pos: 13093571281108186549,
                    length: 13055572161835939253,
                    prop: 8174439530707137973,
                }),
            },
            Sync { from: 181, to: 181 },
            Sync { from: 181, to: 181 },
            Sync { from: 27, to: 181 },
            Handle {
                site: 181,
                target: 181,
                container: 181,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 3941264106,
                    pos: 1982722105925435391,
                    length: 8150137753889872667,
                    prop: 1953184666628070171,
                }),
            },
            Sync { from: 181, to: 181 },
            Sync { from: 181, to: 181 },
            Sync { from: 181, to: 27 },
            Sync { from: 181, to: 181 },
            Sync { from: 181, to: 181 },
            Undo {
                site: 255,
                op_len: 4294967295,
            },
            Handle {
                site: 181,
                target: 181,
                container: 181,
                action: Generic(GenericAction {
                    value: Container(Tree),
                    bool: true,
                    key: 2307503541,
                    pos: 15553137160186484565,
                    length: 8215928015238070273,
                    prop: 1953184666635301490,
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
                    pos: 590711300899,
                    length: 0,
                    prop: 0,
                }),
            },
        ],
    )
}

#[test]
fn gc_fuzz_21() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 11,
                target: 11,
                container: 11,
                action: Generic(GenericAction {
                    value: I32(185335563),
                    bool: true,
                    key: 3385444809,
                    pos: 5714873654208057167,
                    length: 5714873654208057137,
                    prop: 5714873654208057167,
                }),
            },
            Checkout {
                site: 79,
                to: 189747023,
            },
            Handle {
                site: 11,
                target: 38,
                container: 11,
                action: Generic(GenericAction {
                    value: Container(Map),
                    bool: true,
                    key: 185274159,
                    pos: 795741901218843403,
                    length: 795741901218843403,
                    prop: 795741901218843403,
                }),
            },
            Handle {
                site: 11,
                target: 11,
                container: 11,
                action: Generic(GenericAction {
                    value: Container(List),
                    bool: true,
                    key: 189747023,
                    pos: 2748896764614085387,
                    length: 3400217718631893771,
                    prop: 795741901219114799,
                }),
            },
            Handle {
                site: 11,
                target: 11,
                container: 11,
                action: Generic(GenericAction {
                    value: I32(185273099),
                    bool: true,
                    key: 185273099,
                    pos: 795741901218843549,
                    length: 795741901218843403,
                    prop: 5714873654208102155,
                }),
            },
            Handle {
                site: 11,
                target: 11,
                container: 11,
                action: Generic(GenericAction {
                    value: I32(805178123),
                    bool: true,
                    key: 791613439,
                    pos: 795741901218843599,
                    length: 795741901218843403,
                    prop: 795741901218843403,
                }),
            },
            Handle {
                site: 11,
                target: 11,
                container: 11,
                action: Generic(GenericAction {
                    value: I32(185273099),
                    bool: true,
                    key: 185273099,
                    pos: 795741901218843403,
                    length: 18446743021627837195,
                    prop: 795741909623504895,
                }),
            },
            Handle {
                site: 11,
                target: 15,
                container: 11,
                action: Generic(GenericAction {
                    value: I32(185273099),
                    bool: true,
                    key: 1862994699,
                    pos: 795741901224567158,
                    length: 2741296940242897675,
                    prop: 3458764505415879435,
                }),
            },
            Handle {
                site: 11,
                target: 11,
                container: 11,
                action: Generic(GenericAction {
                    value: I32(185273099),
                    bool: true,
                    key: 185273099,
                    pos: 795741901218843403,
                    length: 795741901218843403,
                    prop: 8001501305011637003,
                }),
            },
            Undo {
                site: 111,
                op_len: 1869573999,
            },
            Undo {
                site: 111,
                op_len: 1869573999,
            },
            Undo {
                site: 79,
                op_len: 1330597711,
            },
            Checkout {
                site: 79,
                to: 1330597711,
            },
            Checkout {
                site: 11,
                to: 185273099,
            },
            Handle {
                site: 11,
                target: 11,
                container: 254,
                action: Generic(GenericAction {
                    value: I32(185544495),
                    bool: true,
                    key: 185273099,
                    pos: 795741901218843403,
                    length: 795741901218843403,
                    prop: 4123339075892218635,
                }),
            },
            Handle {
                site: 11,
                target: 11,
                container: 11,
                action: Generic(GenericAction {
                    value: I32(1330597887),
                    bool: true,
                    key: 1330597711,
                    pos: 2741296940242897675,
                    length: 3458764505415879462,
                    prop: 1302123111085387567,
                }),
            },
        ],
    )
}

#[test]
fn gc_fuzz_22() {
    test_multi_sites_with_gc(
        5,
        vec![FuzzTarget::All],
        &mut [
            Handle {
                site: 59,
                target: 27,
                container: 147,
                action: Generic(GenericAction {
                    value: I32(-1819044973),
                    bool: true,
                    key: 16814995,
                    pos: 6590743253515379200,
                    length: 50581795804069894,
                    prop: 506381212763488288,
                }),
            },
            Handle {
                site: 7,
                target: 7,
                container: 7,
                action: Generic(GenericAction {
                    value: I32(1792),
                    bool: false,
                    key: 0,
                    pos: 18446580246477012992,
                    length: 0,
                    prop: 8097874551267853056,
                }),
            },
            SyncAll,
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 2475922323,
                    pos: 761987946344649619,
                    length: 4216591183764737623,
                    prop: 7308324466053836044,
                }),
            },
            Handle {
                site: 59,
                target: 27,
                container: 147,
                action: Generic(GenericAction {
                    value: I32(-1819044973),
                    bool: true,
                    key: 16814995,
                    pos: 6590743253515379200,
                    length: 50581795804069894,
                    prop: 506381212763488288,
                }),
            },
            SyncAll,
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: Container(Text),
                    bool: true,
                    key: 2475922323,
                    pos: 761987946344649619,
                    length: 9583474564706815575,
                    prop: 7809911865117314106,
                }),
            },
            Handle {
                site: 27,
                target: 27,
                container: 27,
                action: Generic(GenericAction {
                    value: I32(-1819082494),
                    bool: true,
                    key: 2475922323,
                    pos: 5644992284945547520,
                    length: 881162909321221975,
                    prop: 6874019576048676717,
                }),
            },
            SyncAllUndo {
                site: 159,
                op_len: 2678038431,
            },
        ],
    )
}

#[test]
fn detached_editing_arb_test() {
    fn prop(u: &mut Unstructured<'_>, site_num: u8) -> arbitrary::Result<()> {
        let xs = u.arbitrary::<Vec<Action>>()?;
        if let Err(e) = std::panic::catch_unwind(|| {
            test_multi_sites_on_one_doc(site_num, &mut xs.clone());
        }) {
            dbg!(xs);
            println!("{:?}", e);
            panic!()
        } else {
            Ok(())
        }
    }

    arbtest::builder().budget_ms(5000).run(|u| prop(u, 5))
}

#[test]
fn detached_editing_failed_case_0() {
    test_multi_sites_on_one_doc(
        5,
        &mut vec![
            Handle {
                site: 41,
                target: 163,
                container: 46,
                action: Generic(GenericAction {
                    value: I32(50529027),
                    bool: true,
                    key: 50529027,
                    pos: 217020518514230019,
                    length: 217020518514230019,
                    prop: 3298585412355,
                }),
            },
            Handle {
                site: 3,
                target: 3,
                container: 3,
                action: Generic(GenericAction {
                    value: I32(-989658365),
                    bool: true,
                    key: 16777215,
                    pos: 562945658454016,
                    length: 18446737545359261695,
                    prop: 17726168133330218751,
                }),
            },
            Handle {
                site: 255,
                target: 255,
                container: 255,
                action: Generic(GenericAction {
                    value: I32(1375731456),
                    bool: true,
                    key: 4294967295,
                    pos: 18446508778221207807,
                    length: 17870283321392431103,
                    prop: 1099511570175,
                }),
            },
            SyncAll,
            Handle {
                site: 3,
                target: 3,
                container: 3,
                action: Generic(GenericAction {
                    value: I32(50529027),
                    bool: true,
                    key: 4294967043,
                    pos: 18446744073709551615,
                    length: 17732919271358463,
                    prop: 4294967237,
                }),
            },
        ],
    )
}

#[test]
fn minify() {
    minify_error(
        5,
        |n, actions| test_multi_sites_with_gc(n, vec![FuzzTarget::All], actions),
        |_, actions| actions.to_vec(),
        vec![],
    )
}
