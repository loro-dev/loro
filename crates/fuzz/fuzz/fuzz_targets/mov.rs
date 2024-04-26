#![no_main]

use libfuzzer_sys::fuzz_target;

use fuzz::{test_multi_sites, Action, FuzzTarget};

fuzz_target!(|actions: Vec<Action>| {
    test_multi_sites(
        5,
        vec![FuzzTarget::Map, FuzzTarget::MovableList],
        &mut actions.clone(),
    )
});
