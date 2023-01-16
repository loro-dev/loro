#![no_main]
use libfuzzer_sys::fuzz_target;
use loro_internal::fuzz::{test_single_client_encode, Action};

fuzz_target!(|data: Vec<Action>| {
    // fuzzed code goes here
    test_single_client_encode(data)
});
