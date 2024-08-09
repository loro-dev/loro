use dev_utils::get_allocated_bytes;
use examples::sheet::init_large_sheet;

pub fn main() {
    let allocated = get_allocated_bytes();
    println!("allocated bytes: {}", allocated);
    let doc = init_large_sheet();
    let allocated = get_allocated_bytes();
    println!("allocated bytes: {}", allocated);
    doc.export_snapshot();
    let allocated = get_allocated_bytes();
    println!("allocated bytes: {}", allocated);
    drop(doc);
    let allocated = get_allocated_bytes();
    println!("allocated bytes: {}", allocated);
}
