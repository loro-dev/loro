#![no_main]

use bench_utils::{draw::DrawAction, Action};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|actions: Vec<Action<DrawAction>>| {
    examples::draw::run_actions_fuzz_in_async_mode(5, 100, &actions)
});
