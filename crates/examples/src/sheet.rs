use loro::{LoroDoc, LoroMap, LoroValue};

pub fn init_large_sheet(size: usize) -> LoroDoc {
    assert!(size >= 100);
    let doc = LoroDoc::new();
    doc.set_peer_id(0).unwrap();
    let rows = doc.get_list("rows");
    for _ in 0..size / 100 {
        let map = rows.push_container(LoroMap::new()).unwrap();
        for i in 0..100 {
            let sub_map = map
                .insert_container(&i.to_string(), LoroMap::new())
                .unwrap();
            sub_map.insert("value", i).unwrap();
            sub_map.insert("meta", LoroValue::Null).unwrap();
        }
    }

    doc
}
