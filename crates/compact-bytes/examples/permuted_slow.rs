use compact_bytes::CompactBytes;
use criterion::black_box;

pub fn main() {
    let data = include_str!("../benches/permuted.mht");
    for _ in 0..1000 {
        let mut bytes = CompactBytes::new();
        bytes.append(&black_box(data).as_bytes()[..data.len() / 2]);
        bytes.alloc_advance(&black_box(&data.as_bytes()[data.len() / 2..]));
    }
}
