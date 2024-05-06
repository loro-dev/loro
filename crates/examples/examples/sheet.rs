use examples::sheet::init_large_sheet;

pub fn main() {
    let doc = init_large_sheet();
    doc.export_snapshot();
}
