use loro::{LoroDoc, LoroMap};

pub fn init_large_sheet() -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(0).unwrap();
    let cols = doc.get_list("cols");
    let rows = doc.get_list("rows");
    for _ in 0..bench_utils::sheet::SheetAction::MAX_ROW {
        let map = rows.push_container(LoroMap::new()).unwrap();
        for i in 0..10 {
            map.insert(&i.to_string(), i).unwrap();
        }
    }

    for i in 0..bench_utils::sheet::SheetAction::MAX_COL {
        cols.push(i as i32).unwrap();
    }

    doc
}
