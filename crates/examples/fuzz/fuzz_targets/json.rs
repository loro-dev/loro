#![no_main]

use bench_utils::{json::JsonAction, Action};
use examples::json::fuzz;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: Vec<Action<JsonAction>>| {
    fuzz(5, &data);
});
