#![no_main]
use libfuzzer_sys::fuzz_target;
use loro_core::fuzz::{test_multi_sites, Action};

fuzz_target!(|actions: Vec<Action>| { test_multi_sites(2, actions) });
