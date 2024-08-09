use dev_utils::get_allocated_bytes;
use examples::sheet::init_large_sheet;
use loro::ID;

pub fn main() {
    let doc = init_large_sheet();
    doc.commit();
    let allocated = get_allocated_bytes();
    println!("Allocated bytes for 10M cells spreadsheet: {}", allocated);
    println!("Has history cache: {}", doc.has_history_cache());

    doc.checkout(&ID::new(doc.peer_id(), 100).into()).unwrap();
    println!(
        "Has history cache after checkout: {}",
        doc.has_history_cache()
    );

    doc.checkout_to_latest();
    let allocated_after_checkout = get_allocated_bytes();
    println!(
        "Allocated bytes after checkout: {}",
        allocated_after_checkout
    );

    let diff = allocated_after_checkout - allocated;
    println!("Checkout history cache size: {}", diff);

    doc.free_history_cache();
    let allocated_after_free = get_allocated_bytes();
    println!(
        "Allocated bytes after free history cache: {}",
        allocated_after_free
    );

    let diff = allocated_after_checkout - allocated_after_free;
    println!("Freed history cache size: {}", diff);
}
