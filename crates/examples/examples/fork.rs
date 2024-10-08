use loro::LoroDoc;

fn main() {
    let mut doc = LoroDoc::new();
    for _ in 0..10_000 {
        let text = doc.get_text("text");
        text.insert(0, "Hi").unwrap();
        doc = doc.fork();
    }
}
