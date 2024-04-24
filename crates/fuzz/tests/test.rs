use arbtest::arbitrary::{self, Unstructured};
use fuzz::{
    actions::{ActionWrapper::*, GenericAction},
    crdt_fuzzer::{test_multi_sites, Action, Action::*, FuzzTarget, FuzzValue::*},
};
use loro::ContainerType::*;
use tracing_subscriber::fmt::format::FmtSpan;

#[ctor::ctor]
fn init_color_backtrace() {
    color_backtrace::install();
    use tracing_subscriber::{prelude::*, registry::Registry};
    if option_env!("DEBUG").is_some() {
        tracing::subscriber::set_global_default(
            Registry::default().with(
                tracing_subscriber::fmt::Layer::default()
                    .with_file(true)
                    .with_line_number(true)
                    .with_span_events(FmtSpan::ACTIVE),
            ),
        )
        .unwrap();
    }
}

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
fn random_fuzz_1s_2sites() {
    arbtest::builder().budget_ms(1000).run(|u| prop(u, 2))
}

#[test]
fn random_fuzz_1s_2sites_1() {
    arbtest::builder().budget_ms(1000).run(|u| prop(u, 2))
}

#[test]
fn random_fuzz_1s_2sites_2() {
    arbtest::builder().budget_ms(1000).run(|u| prop(u, 2))
}

#[test]
fn random_fuzz_1s_5sites() {
    arbtest::builder().budget_ms(1000).run(|u| prop(u, 5))
}

#[test]
fn random_fuzz_1s_5sites_1() {
    arbtest::builder().budget_ms(1000).run(|u| prop(u, 5));
}

#[test]
fn random_fuzz_1s_5sites_2() {
    arbtest::builder().budget_ms(1000).run(|u| prop(u, 5));
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
