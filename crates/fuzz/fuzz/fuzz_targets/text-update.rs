#![no_main]

use libfuzzer_sys::fuzz_target;
use loro::LoroDoc;

fuzz_target!(|data: [&str; 3]| {
    let (old, new, new1) = (data[0], data[1], data[2]);
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.update(old, Default::default()).unwrap();
    text.update(new, Default::default()).unwrap();
    assert_eq!(&text.to_string(), new);
    text.update_by_line(new1, Default::default()).unwrap();
    assert_eq!(&text.to_string(), new1);
});
