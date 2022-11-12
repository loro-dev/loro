#![no_main]
use libfuzzer_sys::fuzz_target;
use loro_core::fuzz::recursive::{test_multi_sites, Action};

fuzz_target!(|actions: Vec<Action>| { test_multi_sites(8, actions) });
