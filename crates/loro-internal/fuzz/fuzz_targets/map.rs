#![no_main]

use libfuzzer_sys::fuzz_target;
use loro_internal::fuzz::crdt_fuzzer::{test_multi_sites, Action, FuzzTarget};

fuzz_target!(|actions: Vec<Action>| {
    test_multi_sites(5, vec![FuzzTarget::Map], &mut actions.clone())
});
