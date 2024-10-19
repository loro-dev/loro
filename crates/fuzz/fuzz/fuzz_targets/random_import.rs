#![no_main]

use fuzz::test_random_bytes_import;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    test_random_bytes_import(data);
});
