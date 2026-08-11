use loro::LoroDoc;
use std::time::Instant;

/// Regression guard for style-anchor accumulation from redundant marks.
///
/// The skip-redundant-marks check in `mark_with_txn` used to work only when
/// the marked range fell inside a single style-range leaf, so on any styled
/// document a caller re-asserting a mark that changes nothing (e.g. an editor
/// binding syncing mark state) recorded a new op every time. Each of those
/// ops leaves a pair of style anchors in the container state forever — they
/// survive snapshots and are never consolidated — and every styled read pays
/// for all of them, so reads degraded without bound. After the fix redundant
/// marks are skipped, so read time should stay flat as `n` grows.
///
/// Run with:
/// cargo test -p loro perf_redundant_marks_do_not_degrade_styled_reads -- --ignored --nocapture
#[test]
#[ignore]
fn perf_redundant_marks_do_not_degrade_styled_reads() {
    fn bench(n: usize) -> std::time::Duration {
        let doc = LoroDoc::new();
        let text = doc.get_text("text");
        text.insert(0, &"x".repeat(724)).unwrap();
        // Fragment the style ranges so the redundant marks below take the
        // spans-multiple-leaves path.
        text.mark(0..100, "bold", true).unwrap();
        text.mark(0..724, "bold", true).unwrap();
        for _ in 0..n {
            text.mark(0..724, "bold", true).unwrap();
        }
        doc.commit();

        let start = Instant::now();
        for _ in 0..100 {
            std::hint::black_box(text.get_richtext_value());
        }
        start.elapsed() / 100
    }

    let mut prev = 0f64;
    for &n in &[0usize, 1000, 2000, 4000] {
        let d = bench(n);
        let us = d.as_secs_f64() * 1e6;
        let ratio = if prev > 0.0 { us / prev } else { 0.0 };
        println!("redundant_marks={n:>5}  read={us:>9.2} us  x_vs_prev={ratio:.2}");
        prev = us;
    }
}
