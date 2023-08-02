use loro_common::ID;
use loro_internal::LoroDoc;

#[test]
fn test_timestamp() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    let mut txn = doc.txn().unwrap();
    text.insert(&mut txn, 0, "123").unwrap();
    txn.commit().unwrap();
    let change = doc
        .oplog()
        .lock()
        .unwrap()
        .get_change_at(ID::new(doc.peer_id(), 0))
        .unwrap();
    assert!(change.timestamp() > 1690966970);
}
