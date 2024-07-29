#![no_main]

use libfuzzer_sys::fuzz_target;
use loro::LoroDoc;

fuzz_target!(|data: [&str; 3]| {
    let (old, new, new1) = (data[0], data[1], data[2]);
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update(old);
    text.update(new);
    assert_eq!(&text.to_string(), new);
    text.update(new1);
    assert_eq!(&text.to_string(), new1);
});
