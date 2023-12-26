use loro_internal::{container::richtext::TextStyleInfoFlag, LoroDoc};

fn main() {
    let doc0 = LoroDoc::new_auto_commit();
    let doc3 = LoroDoc::new_auto_commit();
    let doc4 = LoroDoc::new_auto_commit();
    let text0 = doc0.get_text("text");
    let text3 = doc3.get_text("text");
    let text4 = doc4.get_text("text");
    text0.insert(0, "[18437736874454765568]").unwrap();
    text0.insert(9, "[11156776183901913088]").unwrap();
    text0
        .mark(8, 8 + 28, "bold", true.into(), TextStyleInfoFlag::BOLD)
        .unwrap();
    doc3.import(&doc0.export_snapshot()).unwrap();
    doc4.import(&doc0.export_snapshot()).unwrap();
    text0.insert(24, "[3558932692]").unwrap();
    text0.insert(10, "[18374685380159995904]").unwrap();
    text0
        .mark(60, 60 + 6, "bold", true.into(), TextStyleInfoFlag::BOLD)
        .unwrap();
    text4.insert(0, "[3158382343024284628]").unwrap();
    text3
        .mark(4, 4 + 21, "bold", true.into(), TextStyleInfoFlag::BOLD)
        .unwrap();
    text0
        .mark(3, 3 + 12, "bold", true.into(), TextStyleInfoFlag::BOLD)
        .unwrap();
    text0.insert(78, "[120259084288]").unwrap();
}
