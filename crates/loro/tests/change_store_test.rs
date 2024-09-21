use loro::{LoroDoc, LoroMap, ID};

#[test]
fn test_compact_change_store() {
    let doc = LoroDoc::new();
    doc.set_peer_id(0).unwrap();
    let text = doc.get_text("text");
    for i in 0..100 {
        text.insert(i, "hello").unwrap();
    }

    let list = doc.get_list("list");
    for _ in 0..100 {
        let map = list.push_container(LoroMap::new()).unwrap();
        for j in 0..100 {
            map.insert(&j.to_string(), j).unwrap();
        }
    }

    doc.commit();
    doc.compact_change_store();
    doc.checkout(&ID::new(0, 60).into()).unwrap();
}
