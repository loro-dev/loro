use bytes::Bytes;
use loro_kv_store::sstable::SsTable;

#[test]
fn import_moon_encoded_sstable() {
    let bytes = Bytes::from_static(include_bytes!("testdata/moon_sstable_simple.bin"));
    let table = SsTable::import_all(bytes, true).unwrap();
    let kvs: Vec<(Bytes, Bytes)> = table.iter().collect();

    assert_eq!(
        kvs,
        vec![
            (Bytes::from_static(b"a"), Bytes::from_static(b"1")),
            (Bytes::from_static(b"ab"), Bytes::from_static(b"2")),
            (Bytes::from_static(b"z"), Bytes::from_static(b"")),
        ]
    );
}

