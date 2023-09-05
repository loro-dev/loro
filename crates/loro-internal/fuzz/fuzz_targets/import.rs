#![no_main]
use libfuzzer_sys::fuzz_target;
use loro_internal::LoroDoc;

fuzz_target!(|data: Vec<u8>| {
    let mut doc = LoroDoc::default();
    doc.import(&data);
});
