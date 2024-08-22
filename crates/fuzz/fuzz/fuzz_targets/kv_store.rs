#![no_main]

use fuzz::{test_mem_kv_fuzzer, KVAction};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|actions: Vec<KVAction>| {
    test_mem_kv_fuzzer(&mut actions);
});
