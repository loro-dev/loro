use arbtest::arbitrary::{self, Unstructured};
use fuzz::crdt_fuzzer::{test_multi_sites, Action, FuzzTarget};

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
