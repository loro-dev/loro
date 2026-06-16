use loro::LoroDoc;
use std::time::Instant;

/// Regression guard for the O(n^2) text-edit blow-up that shipped in 1.12.0.
///
/// Editing with UTF-16 / UTF-8 (byte) coordinates used to validate every
/// position by materializing the whole `[0, pos)` prefix string, making each
/// edit O(n) and a run of edits O(n^2). After the fix the boundary check is
/// O(log n), so the cost should scale ~linearly.
///
/// Run with:
/// cargo test -p loro perf_text_insert_utf16_is_linear -- --ignored --nocapture
#[test]
#[ignore]
fn perf_text_insert_utf16_is_linear() {
    fn bench(n: usize) -> std::time::Duration {
        let doc = LoroDoc::new();
        let text = doc.get_text("text");
        let mut seed: u64 = 42;
        let mut rnd = || {
            seed = (seed.wrapping_mul(1103515245).wrapping_add(12345)) & 0x7fffffff;
            seed as f64 / 0x7fffffff as f64
        };
        let start = Instant::now();
        for _ in 0..n {
            let len = text.len_utf16();
            let pos = (rnd() * (len + 1) as f64).floor() as usize;
            text.insert_utf16(pos, "x").unwrap();
        }
        doc.commit();
        start.elapsed()
    }

    let mut prev = 0f64;
    for &n in &[6000usize, 12000, 24000, 48000] {
        let d = bench(n);
        let ms = d.as_secs_f64() * 1000.0;
        let ratio = if prev > 0.0 { ms / prev } else { 0.0 };
        println!(
            "n={n:>6}  {ms:>9.1} ms  per_op={:>7.3}us  x_for_2x_work={ratio:.2}",
            d.as_secs_f64() / n as f64 * 1e6
        );
        prev = ms;
    }
}
