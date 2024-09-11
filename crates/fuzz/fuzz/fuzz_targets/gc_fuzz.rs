#![no_main]

use libfuzzer_sys::fuzz_target;

use fuzz::{test_multi_sites_with_gc, Action, FuzzTarget};

fuzz_target!(|actions: Vec<Action>| {
    test_multi_sites_with_gc(5, vec![FuzzTarget::All], &mut actions.clone());
});
