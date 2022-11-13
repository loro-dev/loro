#![no_main]
use libfuzzer_sys::fuzz_target;
use loro_core::fuzz::recursive::{test_multi_sites, Action};

fuzz_target!(|actions: [Action; 100]| { test_multi_sites(5, &mut actions.clone()) });
