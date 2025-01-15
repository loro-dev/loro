#![allow(unexpected_cfgs)]
use bytes::Bytes;
use loro_kv_store::{mem_store::MemKvConfig, MemKvStore};

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

#[test]
fn add_and_remove() {
    let key = &[0];
    let value = Bytes::from_static(&[0]);
    let mut store = MemKvStore::new(MemKvConfig::default());
    store.set(key, value.clone());
    assert_eq!(store.get(key), Some(value));
    store.remove(key);
    assert_eq!(store.get(key), None);
}

#[test]
fn add_flush_remove() {
    let key = &[0];
    let value = Bytes::from_static(&[0]);
    let mut store = MemKvStore::new(MemKvConfig::default());
    store.set(key, value.clone());
    store.export_all();
    store.remove(key);
    assert_eq!(store.get(key), None);
}

#[test]
fn add_flush_add_scan() {
    let key1 = &[0];
    let value1 = Bytes::from_static(&[0]);
    let key2 = &[128];
    let value2 = Bytes::from_static(&[252, 169]);
    let mut store = MemKvStore::new(MemKvConfig::new().should_encode_none(true));
    store.set(key1, value1.clone());
    store.export_all();
    ensure_cov::assert_cov("kv_store::block::NormalBlock::encode::compress_fallback");
    store.set(key2, value2.clone());
    {
        let mut iter = store.scan(std::ops::Bound::Unbounded, std::ops::Bound::Unbounded);
        assert_eq!(
            iter.next(),
            Some((Bytes::from_static(key1), value1.clone()))
        );
        assert_eq!(
            iter.next(),
            Some((Bytes::from_static(key2), value2.clone()))
        );
        assert_eq!(iter.next(), None);

        let mut iter = store
            .scan(std::ops::Bound::Unbounded, std::ops::Bound::Unbounded)
            .rev();
        assert_eq!(
            iter.next(),
            Some((Bytes::from_static(key2), value2.clone()))
        );
        assert_eq!(
            iter.next(),
            Some((Bytes::from_static(key1), value1.clone()))
        );
        assert_eq!(iter.next(), None);
    }

    let bytes = store.export_all();
    let mut store = MemKvStore::new(MemKvConfig::new());
    store.import_all(bytes).unwrap();
    let mut iter = store.scan(std::ops::Bound::Unbounded, std::ops::Bound::Unbounded);
    assert_eq!(
        iter.next(),
        Some((Bytes::from_static(key1), value1.clone()))
    );
    assert_eq!(
        iter.next(),
        Some((Bytes::from_static(key2), value2.clone()))
    );
    assert_eq!(iter.next(), None);
}

#[test]
fn large_value() {
    use rand::Rng;
    let key = &[0];
    let mut rng = rand::thread_rng();
    let large_value: Vec<u8> = (0..100_000).map(|_| rng.gen()).collect();
    let large_value = Bytes::from(large_value);

    let mut store = MemKvStore::new(MemKvConfig::new());
    store.set(key, large_value.clone());

    let bytes = store.export_all();
    ensure_cov::assert_cov("kv_store::block::LargeValueBlock::encode::compress_fallback");
    let mut imported_store = MemKvStore::new(MemKvConfig::new());
    imported_store.import_all(bytes).unwrap();

    let retrieved_value = imported_store.get(key).unwrap();
    assert_eq!(retrieved_value, large_value);

    let mut iter = imported_store.scan(std::ops::Bound::Unbounded, std::ops::Bound::Unbounded);
    assert_eq!(iter.next(), Some((Bytes::from_static(key), large_value)));
    assert_eq!(iter.next(), None);
}

#[test]
fn export_import_edit_export() {
    let mut store = MemKvStore::new(MemKvConfig::default());
    store.set(b"a", Bytes::from_static(b"1"));
    for i in 0..3000 {
        let s = format!("b{}", i);
        store.set(s.as_bytes(), Bytes::from_static(b"2"));
    }

    let bytes = store.export_all();
    let mut new_store = MemKvStore::new(MemKvConfig::default());
    new_store.import_all(bytes).unwrap();
    new_store.set(b"a", Bytes::from_static(b"2"));
    assert_eq!(new_store.get(b"b0"), Some(Bytes::from_static(b"2")));
    assert_eq!(new_store.get(b"b1001"), Some(Bytes::from_static(b"2")));
    assert_eq!(new_store.get(b"b2999"), Some(Bytes::from_static(b"2")));

    let bytes = new_store.export_all();
    let mut new_new_store = MemKvStore::new(MemKvConfig::default());
    new_new_store.import_all(bytes).unwrap();
    assert_eq!(new_new_store.get(b"b0"), Some(Bytes::from_static(b"2")));
    assert_eq!(new_new_store.get(b"b1001"), Some(Bytes::from_static(b"2")));
    assert_eq!(new_new_store.get(b"b2999"), Some(Bytes::from_static(b"2")));
    assert_eq!(new_new_store.get(b"b99"), Some(Bytes::from_static(b"2")));
    assert_eq!(new_new_store.get(b"a"), Some(Bytes::from_static(b"2")));
}
