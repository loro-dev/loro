use dev_utils::{get_mem_usage, ByteSize};
use examples::sheet::init_large_sheet;
use loro::ID;

pub fn main() {
    let doc = init_large_sheet(10_000_000);
    // let doc = init_large_sheet(10_000);
    doc.commit();
    let allocated = get_mem_usage();
    println!("Allocated bytes for 10M cells spreadsheet: {}", allocated);
    println!("Has history cache: {}", doc.has_history_cache());

    doc.checkout(&ID::new(doc.peer_id(), 100).into()).unwrap();
    println!(
        "Has history cache after checkout: {}",
        doc.has_history_cache()
    );

    doc.checkout_to_latest();
    let after_checkout = get_mem_usage();
    println!("Allocated bytes after checkout: {}", after_checkout);

    doc.free_diff_calculator();
    let after_free_diff_calculator = get_mem_usage();
    println!(
        "Allocated bytes after freeing diff calculator: {}",
        after_free_diff_calculator
    );

    println!(
        "Diff calculator size: {}",
        after_checkout - after_free_diff_calculator
    );

    doc.free_history_cache();
    let after_free_history_cache = get_mem_usage();
    println!(
        "Allocated bytes after free history cache: {}",
        after_free_history_cache
    );

    println!(
        "History cache size: {}",
        after_free_diff_calculator - after_free_history_cache
    );

    doc.compact_change_store();
    let after_compact_change_store = get_mem_usage();
    println!(
        "Allocated bytes after compact change store: {}",
        after_compact_change_store
    );
    println!(
        "Shrink change store size: {}",
        after_free_history_cache - after_compact_change_store
    );

    let snapshot = doc.export_snapshot();
    println!("Snapshot size: {}", ByteSize(snapshot.len()));
}
