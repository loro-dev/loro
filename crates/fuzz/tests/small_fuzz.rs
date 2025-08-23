use arbtest::arbitrary::{self, Unstructured};
use fuzz::{
    actions::GenericAction,
    crdt_fuzzer::{test_multi_sites, Action, FuzzTarget},
};
use Action::*;

fn prop(u: &mut Unstructured<'_>, site_num: u8) -> arbitrary::Result<()> {
    let xs = u.arbitrary::<Vec<Action>>()?;
    if let Err(e) = std::panic::catch_unwind(|| {
        test_multi_sites(site_num, vec![FuzzTarget::All], &mut xs.clone());
    }) {
        dbg!(xs);
        println!("{e:?}");
        panic!()
    } else {
        Ok(())
    }
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

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

#[test]
fn a_failed_case() {
    test_multi_sites(
        5,
        vec![FuzzTarget::All],
        &mut [Handle {
            site: 220,
            target: 142,
            container: 63,
            action: fuzz::actions::ActionWrapper::Generic(GenericAction {
                value: fuzz::actions::FuzzValue::I32(-475812747),
                bool: true,
                key: 3772263912,
                pos: 5074681398301933407,
                length: 14215598495239327317,
                prop: 11034577274858974974,
            }),
        }],
    );
}
