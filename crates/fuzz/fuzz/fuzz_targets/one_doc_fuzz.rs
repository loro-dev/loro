#![no_main]

use libfuzzer_sys::fuzz_target;

use fuzz::{test_multi_sites_on_one_doc, Action, FuzzTarget};

fuzz_target!(|actions: Vec<Action>| {
    test_multi_sites_on_one_doc(5, &mut actions.clone());
});
