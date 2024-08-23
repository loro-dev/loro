use fuzz::{test_mem_kv_fuzzer, KVAction::*};

#[test]
fn add_same_key_twice() {
    test_mem_kv_fuzzer(&mut [
        Add {
            key: vec![],
            value: vec![254],
        },
        Flush,
        Add {
            key: vec![],
            value: vec![],
        },
    ])
}

#[test]
fn add_and_remove() {
    test_mem_kv_fuzzer(&mut [
        Add {
            key: vec![],
            value: vec![238],
        },
        Remove(0),
    ])
}

#[test]
fn add_flush_remove() {
    test_mem_kv_fuzzer(&mut [
        Add {
            key: vec![],
            value: vec![],
        },
        Flush,
        Remove(3791655167),
    ])
}
