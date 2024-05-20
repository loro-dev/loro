#![no_main]

use libfuzzer_sys::fuzz_target;
use loro_delta_fuzz::{run, Op};

fuzz_target!(|ops: Vec<Op>| run(ops, 5));
