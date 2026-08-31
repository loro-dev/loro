use loro::{Frontiers, LoroDoc};
use std::time::{Duration, Instant};

/// Regression guard for the O((len/256)^2) checkout across a single large
/// text op.
///
/// A single op of length L is stored in `IdToCursor` as L/256 fragments, so a
/// checkout across the op emits one `LeafUpdate` per fragment - all targeting
/// the same rope leaf with the same status change. `CrdtRope::update` used to
/// split the leaf at every fragment boundary, after which the parts merged
/// straight back together, and the caller re-mapped every returned leaf over
/// the whole op span: a 1.2M-char paste took seconds to checkout across,
/// while the insert itself took milliseconds. With contiguous same-effect
/// updates coalesced before splitting, the checkout is a few milliseconds.
///
/// Run with:
/// cargo test -p loro perf_checkout_across_giant_text_op -- --ignored --nocapture
///
/// You can scale it with:
/// LORO_PERF_CHARS=4800000 cargo test -p loro perf_checkout_across_giant_text_op -- --ignored --nocapture
#[test]
#[ignore]
fn perf_checkout_across_giant_text_op() {
    let chars: usize = std::env::var("LORO_PERF_CHARS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_200_000);

    let doc = LoroDoc::new();
    let text = doc.get_text("t");
    let content = "lorem ipsum ".repeat(chars / 12);
    let len = content.chars().count();

    let start = Instant::now();
    text.insert(0, &content).unwrap();
    doc.commit();
    let insert_elapsed = start.elapsed();

    let latest = doc.oplog_frontiers();

    let start = Instant::now();
    doc.checkout(&Frontiers::default()).unwrap();
    let to_empty_elapsed = start.elapsed();
    assert_eq!(doc.get_text("t").len_unicode(), 0);

    let start = Instant::now();
    doc.checkout(&latest).unwrap();
    let to_latest_elapsed = start.elapsed();
    assert_eq!(doc.get_text("t").len_unicode(), len);

    println!(
        "perf_checkout_across_giant_text_op: chars={}, insert={:?}, checkout_to_empty={:?}, checkout_to_latest={:?}",
        len, insert_elapsed, to_empty_elapsed, to_latest_elapsed
    );

    // The quadratic path cost 3-12s at the default size; the fixed path costs
    // a few milliseconds. The bound is deliberately loose so slow CI machines
    // never trip it while a regression to quadratic always does.
    let bound = Duration::from_secs(2);
    assert!(
        to_empty_elapsed < bound && to_latest_elapsed < bound,
        "checkout across a single {len}-char op is pathologically slow: to_empty={to_empty_elapsed:?}, to_latest={to_latest_elapsed:?}"
    );
}
