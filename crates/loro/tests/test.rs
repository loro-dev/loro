use loro::Loro;

#[test]
fn input_text() {
    let mut doc = Loro::new(Default::default(), None);
    let mut text = doc.get_text("text");
    doc.txn(|txn| {
        text.insert(&txn, 0, "123").unwrap();
    });
    let mut doc_b = Loro::new(Default::default(), None);
    doc_b.txn(|mut txn| {
        txn.decode(&doc.encode_all()).unwrap();
    });

    let a = doc.to_json();
    assert_eq!(a, doc_b.to_json());
}
