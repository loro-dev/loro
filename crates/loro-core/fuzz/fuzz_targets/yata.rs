#![no_main]
use crdt_list::{test, test::Action};
use libfuzzer_sys::fuzz_target;
use loro_core::container::text::tracker::yata::YataImpl;

fuzz_target!(|data: Vec<Action>| { test::test_with_actions::<YataImpl>(5, &data) });
