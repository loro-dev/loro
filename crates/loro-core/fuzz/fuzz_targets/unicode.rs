#![no_main]

use libfuzzer_sys::fuzz_target;
use loro_core::text::{apply, Action};

fuzz_target!(|data: Vec<Action>| {
    // fuzzed code goes here
    let mut data = data;
    apply(&mut data);
});
