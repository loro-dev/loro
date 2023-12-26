use loro_common::LoroValue;
use loro_internal::{container::richtext::TextStyleInfoFlag, LoroDoc};

fn main() {
    let doc = LoroDoc::new_auto_commit();
    let text = doc.get_text("text");
    text.insert(0, "123").unwrap();
    text.mark(0, 2, "bold", LoroValue::Null, TextStyleInfoFlag::BOLD)
        .unwrap();
    text.insert(2, "456").unwrap();
    text.mark(
        5,
        6,
        "bold",
        LoroValue::Null,
        TextStyleInfoFlag::BOLD.to_delete(),
    )
    .unwrap();
    text.insert(6, "abc").unwrap();
    println!("{:?}", text.get_value());
    println!("{:?}", text.get_richtext_value());
}
