use loro::{LoroDoc, LoroList, LoroMap};

pub struct Actor {
    pub doc: LoroDoc,
    cols: LoroList,
    rows: LoroList,
}

impl Actor {}

pub fn init_sheet() -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(0).unwrap();
    let cols = doc.get_list("cols");
    let rows = doc.get_list("rows");
    for i in 0..bench_utils::sheet::SheetAction::MAX_ROW {
        rows.push_container(loro::ContainerType::Map).unwrap();
    }

    for i in 0..bench_utils::sheet::SheetAction::MAX_COL {
        cols.push(i as i32).unwrap();
    }

    doc
}
