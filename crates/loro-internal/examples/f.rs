

use loro_internal::LoroDoc;

fn main() {
    let snapshot = std::fs::read("/Users/leon/Desktop/debug/snapshot_with_unknown").unwrap();
    let updates = std::fs::read("/Users/leon/Desktop/debug/updates_with_unknown").unwrap();
    let doc = LoroDoc::new_auto_commit();
    doc.import(&updates).unwrap();
    println!("{:?}", doc.get_deep_value());

    let doc2 = LoroDoc::new_auto_commit();
    doc2.import(&snapshot).unwrap();
    println!("{:?}", doc2.get_deep_value());

    let snapshot_with_unknown = doc.export_snapshot();
    let updates_with_unknown = doc.export_from(&Default::default());

    let doc3 = LoroDoc::new_auto_commit();
    doc3.import(&snapshot_with_unknown).unwrap();
    println!("{:?}", doc3.get_deep_value());

    let doc4 = LoroDoc::new_auto_commit();
    doc4.import(&updates_with_unknown).unwrap();
    println!("{:?}", doc4.get_deep_value());
}
