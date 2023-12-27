use loro_internal::LoroDoc;

fn main() {
    let doc1 = LoroDoc::new_auto_commit();
    doc1.set_peer_id(1).unwrap();
    let doc2 = LoroDoc::new_auto_commit();
    doc2.set_peer_id(2).unwrap();
    let text1 = doc1.get_text("text");
    let text2 = doc2.get_text("text");
    text1.insert(0, "Hello, world!").unwrap();
    doc1.commit_then_renew();
    println!(
        "first {:?} {:?}",
        doc1.oplog_frontiers(),
        doc1.get_deep_value()
    );
    doc1.checkout(&Default::default()).unwrap();
    doc1.attach();
    println!(
        "second {:?} {:?}",
        doc1.oplog_frontiers(),
        doc1.get_deep_value()
    );
    text2.insert(0, "123").unwrap();
    doc2.commit_then_renew();
    doc1.import(&doc2.export_snapshot()).unwrap();
    doc2.import(&doc1.export_snapshot()).unwrap();
    println!("text {:?}", doc2.get_deep_value());
    let f = doc1.oplog_frontiers();
    println!(
        "third {:?} {:?}",
        doc1.oplog_frontiers(),
        doc1.get_deep_value()
    );
    doc1.checkout(&Default::default()).unwrap();
    println!("\n\n\n\n\n");
    doc1.checkout(&f).unwrap();
    println!(
        "fourth {:?} {:?}",
        doc1.oplog_frontiers(),
        doc1.get_deep_value()
    );
    let doc3 = LoroDoc::new();
    doc3.import(&doc1.export_from(&Default::default())).unwrap();
    println!(
        "fifth {:?} {:?}",
        doc3.oplog_frontiers(),
        doc3.get_deep_value()
    );
    // doc3.checkout(&Default::default()).unwrap();
    // doc3.checkout(&f).unwrap();
    // println!(
    //     "sixth {:?} {:?}",
    //     doc3.oplog_frontiers(),
    //     doc3.get_deep_value()
    // );
}
