#![no_main]

use libfuzzer_sys::fuzz_target;
use loro_internal::dag::{test_alloc, Interaction};

fuzz_target!(|data: Vec<Interaction>| {
    test_alloc(10, data);
});
