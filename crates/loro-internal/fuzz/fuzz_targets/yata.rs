#![no_main]
use crdt_list::{test, test::Action};
use libfuzzer_sys::fuzz_target;
use loro_internal::container::text::tracker::yata_impl::YataImpl;

fuzz_target!(|data: Vec<Action>| { test::test_with_actions::<YataImpl>(5, 100, data) });
