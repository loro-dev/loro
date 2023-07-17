#![no_main]
use libfuzzer_sys::fuzz_target;
use loro_internal::fuzz::recursive_refactored::{test_multi_sites, Action};

fuzz_target!(|actions: Vec<Action>| { test_multi_sites(5, &mut actions.clone()) });
