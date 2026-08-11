use loro::LoroDoc;
use std::time::Instant;

/// Regression guard for the O(n^2) styled-read blow-up caused by cloning
/// `Styles` once per style range.
///
/// `RichtextState::iter` / `slice_delta` used to build the per-range
/// `StyleMeta` with `x.1.clone().into()`. `Styles` owns a
/// `BTreeSet<Arc<StyleOp>>` per key, holding *every* style op that covers the
/// range, while `StyleMeta` keeps only the LWW winner (`StyleValue::get`).
/// So each range deep-copied a set whose size grows with the number of marks
/// on the container, and then threw all but one element away: O(marks) per
/// range times O(marks) ranges. `From<&Styles> for StyleMeta` already takes a
/// reference, so the copy was pure waste.
///
/// Style anchors are never consolidated, so the marks accumulate in state and
/// every styled read pays for all of them. Measured here:
///
/// | accumulated marks | before  | after |
/// |------------------:|--------:|------:|
/// |               500 |   4.9ms |  82us |
/// |              1000 |  22.6ms | 246us |
/// |              2000 | 103.2ms | 1.0ms |
///
/// This guards the *read* path only, so the growth is still superlinear: the
/// residual comes from `StyleRangeMap` materializing the full op set on every
/// element it covers, which is O(n^2) in memory (309MB at n=4000 for 724
/// visible chars) and is not addressed here.
///
/// Run with:
/// cargo test -p loro perf_styled_read_scales_with_accumulated_marks --release -- --ignored --nocapture
#[test]
#[ignore]
fn perf_styled_read_scales_with_accumulated_marks() {
    fn bench(n: usize) -> std::time::Duration {
        let doc = LoroDoc::new();
        let text = doc.get_text("text");
        text.insert(0, &"x".repeat(724)).unwrap();
        // Alternating mark/unmark over the whole range: every op genuinely
        // changes the styles, so none of them can be skipped as redundant.
        for i in 0..n {
            if i % 2 == 0 {
                text.mark(0..724, "bold", true).unwrap();
            } else {
                text.unmark(0..724, "bold").unwrap();
            }
        }
        doc.commit();

        let start = Instant::now();
        for _ in 0..50 {
            std::hint::black_box(text.get_richtext_value());
        }
        start.elapsed() / 50
    }

    let mut prev = 0f64;
    for &n in &[250usize, 500, 1000, 2000] {
        let d = bench(n);
        let us = d.as_secs_f64() * 1e6;
        let ratio = if prev > 0.0 { us / prev } else { 0.0 };
        println!("accumulated_marks={n:>5}  read={us:>10.2} us  x_for_2x_work={ratio:.2}");
        prev = us;
    }
}
