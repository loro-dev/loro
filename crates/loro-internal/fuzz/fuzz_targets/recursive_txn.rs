#![no_main]
use libfuzzer_sys::fuzz_target;
use loro_internal::fuzz::recursive_txn::{test_multi_sites, Action};

fuzz_target!(|actions: [Action; 100]| { test_multi_sites(5, &mut actions.clone()) });
