use arbtest::arbitrary::{self, Unstructured};
use fuzz::{
    actions::{ActionWrapper::*, GenericAction},
    crdt_fuzzer::{test_multi_sites, Action, Action::*, FuzzTarget, FuzzValue::*},
};
use loro::ContainerType::*;

fn prop(u: &mut Unstructured<'_>, site_num: u8) -> arbitrary::Result<()> {
    let xs = u.arbitrary::<Vec<Action>>()?;
    if let Err(e) = std::panic::catch_unwind(|| {
        test_multi_sites(site_num, vec![FuzzTarget::All], &mut xs.clone());
    }) {
        dbg!(xs);
        println!("{:?}", e);
        panic!()
    } else {
        Ok(())
    }
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
fn fuzz_10s() {
    arbtest::builder()
        .budget_ms((1000 * 10) as u64)
        .run(|u| prop(u, 2))
}
