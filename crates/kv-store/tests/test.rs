use bytes::Bytes;
use loro_kv_store::{mem_store::MemKvConfig, MemKvStore};

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
    store.set(key2, value2.clone());
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
