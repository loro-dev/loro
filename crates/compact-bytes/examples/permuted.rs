use compact_bytes::CompactBytes;

pub fn main() {
    let data = include_str!("../benches/permuted.mht");
    // this simulate the situation in loro snapshot encoding,
    // where we first encode the state snapshot, then we look up the slices of the ops.
    let mut bytes = CompactBytes::with_capacity(data.len());
    println!("{}", bytes.capacity());
    bytes.append(&data.as_bytes()[..data.len() / 2]);
    println!("{}", bytes.as_bytes().len()); // 114275
    bytes.alloc_advance(&data.as_bytes()[data.len() / 2..]);
    println!("{}", bytes.as_bytes().len()); // 117026
}
