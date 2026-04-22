use std::fs;

use arbitrary::{Arbitrary, Unstructured};
use fuzz::{test_multi_sites_on_one_doc, Action};

#[test]
#[ignore]
fn repro_crash_8ba01() {
    let bytes = fs::read("tests/crash_8ba01").unwrap();
    let mut u = Unstructured::new(&bytes);
    let actions: Vec<Action> = Vec::arbitrary_take_rest(u).unwrap();
    let mut actions = actions;
    test_multi_sites_on_one_doc(5, &mut actions);
}
