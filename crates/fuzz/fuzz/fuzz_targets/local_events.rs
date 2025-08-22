#![no_main]

use libfuzzer_sys::fuzz_target;
use loro::LoroDoc;

use fuzz::{fuzz_local_events, Action, FuzzTarget};

fuzz_target!(|actions: Vec<Action>| {
    fuzz_local_events(actions);
});
