#![no_main]

use arbitrary::Arbitrary;
use compact_bytes::CompactBytes;

use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
struct Op {
    data: Vec<u8>,
}

fuzz_target!(|data: Vec<Op>| {
    let mut bytes = CompactBytes::new();
    for op in data {
        let segments = bytes.alloc_advance(&op.data);
        let mut index = 0;
        for seg in segments.iter() {
            assert_eq!(
                bytes.as_bytes()[seg.start..seg.end],
                op.data[index..index + seg.len()]
            );
            index += seg.len();
        }
    }
});
