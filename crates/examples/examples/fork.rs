use criterion::black_box;
use loro::{LoroDoc, LoroText};

fn main() {
    let snapshot = {
        let doc = LoroDoc::new();
        let map = doc.get_map("map");
        for i in 0..10000 {
            let text = map
                .insert_container(&i.to_string(), LoroText::new())
                .unwrap();
            text.insert(0, &i.to_string()).unwrap();
        }
        doc.export(loro::ExportMode::Snapshot).unwrap()
    };
    let mut doc = LoroDoc::new();
    doc.import(&snapshot).unwrap();
    for _ in 0..1000 {
        doc.get_text("text").insert(0, "123").unwrap();
        doc = black_box(doc.fork());
    }
    ensure_cov::assert_cov("kv-store::mem_store::export_with_encoded_block");
}
