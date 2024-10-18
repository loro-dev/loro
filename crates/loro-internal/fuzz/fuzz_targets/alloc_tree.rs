#![no_main]

use libfuzzer_sys::fuzz_target;
use loro_internal::dag::{fuzz_alloc_tree, Interaction};

fuzz_target!(|data: Vec<Interaction>| {
    fuzz_alloc_tree(10, data.clone());
    fuzz_alloc_tree(5, data.clone());
    fuzz_alloc_tree(2, data);
});
