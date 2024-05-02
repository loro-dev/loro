use examples::sheet::init_large_sheet;
use std::time::Instant;

pub fn main() {
    let start = Instant::now();
    let doc = init_large_sheet();
    let init_duration = start.elapsed().as_secs_f64() * 1000.;
    println!("init_duration {}", init_duration);

    let start = Instant::now();
    let snapshot = doc.export_snapshot();
    let duration = start.elapsed().as_secs_f64() * 1000.;
    println!("export duration {} size={}", duration, snapshot.len());
}
