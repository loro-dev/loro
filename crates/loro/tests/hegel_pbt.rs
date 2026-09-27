//! Property-based tests using Hegel.

use hegel::generators as gs;
use hegel::Generator;
use hegel::PrettyPrinter;
use hegel::TestCase;
use loro::{
    ExportMode, LoroCounter, LoroDoc, LoroList, LoroMap, LoroMovableList, LoroText, LoroValue,
};
use pretty_assertions::assert_eq;

/// Short text used for text inserts and string values.
#[hegel::composite]
fn short_text(tc: &TestCase) -> String {
    tc.draw(gs::text().max_size(8))
}

/// Draws a primitive `LoroValue`. Using `draw_silent` avoids a requirement
/// for our generators to be PrettyPrintable, and as draws within composites
/// are not printed anyway, this has no adverse effect.
#[hegel::composite]
fn value(tc: &TestCase) -> LoroValue {
    tc.draw_silent(hegel::one_of!(
        gs::integers::<i64>().map(LoroValue::from),
        gs::booleans().map(LoroValue::from),
        short_text().map(LoroValue::from),
        gs::just(LoroValue::Null),
    ))
}

/// A map key, biased towards a small pool so concurrent edits collide.
#[hegel::composite]
fn key(tc: &TestCase) -> String {
    tc.draw(hegel::one_of!(
        gs::sampled_from(vec!["a", "b", "c", "d"]).map(str::to_string),
        gs::text().max_size(8),
    ))
}

/// A position in `0..=len`.
#[hegel::composite]
fn position(tc: &TestCase, len: usize) -> usize {
    tc.draw(gs::integers::<usize>().max_value(len))
}

/// A non-empty `(pos, count)` delete range within `len > 0` items.
#[hegel::composite]
fn delete_range(tc: &TestCase, len: usize) -> (usize, usize) {
    let pos = tc.draw(position(len - 1));
    let count = tc.draw(gs::integers::<usize>().min_value(1).max_value(len - pos));
    (pos, count)
}

/// Accessing all of the root containers ensures deep value
/// comparisons pass for the same doc (accessing root containers is not
/// stored as an op)
fn touch_roots(doc: &LoroDoc) {
    let _ = doc.get_text("text");
    let _ = doc.get_list("list");
    let _ = doc.get_movable_list("mlist");
    let _ = doc.get_map("map");
    let _ = doc.get_counter("counter");
}

/// Assert two docs agree on version vector, frontiers, and deep value.
/// History is checked first to separate sync bugs from state bugs.
fn assert_docs_agree(a: &LoroDoc, b: &LoroDoc) {
    assert_eq!(a.oplog_vv(), b.oplog_vv());
    assert_eq!(a.oplog_frontiers(), b.oplog_frontiers());
    assert_eq!(a.get_deep_value(), b.get_deep_value());
}

/// Apply one randomly drawn edit to one of the doc's root containers.
fn apply_random_edit(tc: &TestCase, doc: &LoroDoc) {
    // {insert, delete} at random positions
    #[hegel::composite]
    fn edit_text(tc: &TestCase, text: &LoroText) -> () {
        let len = text.len_unicode();
        if len == 0 || tc.draw(gs::booleans()) {
            let pos = tc.draw(position(len));
            let s = tc.draw(short_text());
            text.insert(pos, &s).unwrap();
        } else {
            let (pos, count) = tc.draw(delete_range(len));
            text.delete(pos, count).unwrap();
        }
    }

    // {insert, delete} at random positions
    #[hegel::composite]
    fn edit_list(tc: &TestCase, list: &LoroList) -> () {
        let len = list.len();
        if len == 0 || tc.draw(gs::booleans()) {
            let pos = tc.draw(position(len));
            let v = tc.draw_silent(value());
            list.insert(pos, v).unwrap();
        } else {
            let (pos, count) = tc.draw(delete_range(len));
            list.delete(pos, count).unwrap();
        }
    }

    // {insert, delete, set, mov} at random positions
    #[hegel::composite]
    fn edit_movable_list(tc: &TestCase, mlist: &LoroMovableList) -> () {
        let len = mlist.len();
        // only inserts are possible on an empty list
        let choice = if len == 0 {
            0
        } else {
            tc.draw(gs::integers::<u8>().max_value(3))
        };
        match choice {
            0 => {
                let pos = tc.draw(position(len));
                let v = tc.draw_silent(value());
                mlist.insert(pos, v).unwrap();
            }
            1 => {
                let (pos, count) = tc.draw(delete_range(len));
                mlist.delete(pos, count).unwrap();
            }
            2 => {
                let from = tc.draw(position(len - 1));
                let to = tc.draw(position(len - 1));
                mlist.mov(from, to).unwrap();
            }
            _ => {
                let pos = tc.draw(position(len - 1));
                let v = tc.draw_silent(value());
                mlist.set(pos, v).unwrap();
            }
        }
    }

    // {insert, delete} at random keys
    #[hegel::composite]
    fn edit_map(tc: &TestCase, map: &LoroMap) -> () {
        let key = tc.draw(key());
        if tc.draw(gs::booleans()) {
            let v = tc.draw_silent(value());
            map.insert(&key, v).unwrap();
        } else {
            // deleting a missing key is Ok(())
            map.delete(&key).unwrap();
        }
    }

    // Use integers to avoid issues with non-associative float addition
    // increments with random pos/neg integers
    #[hegel::composite]
    fn edit_counter(tc: &TestCase, counter: &LoroCounter) -> () {
        let v = tc.draw(gs::integers::<i32>().min_value(-1000).max_value(1000));
        counter.increment(v as f64).unwrap();
    }

    tc.draw(hegel::one_of!(
        edit_text(&doc.get_text("text")),
        edit_list(&doc.get_list("list")),
        edit_movable_list(&doc.get_movable_list("mlist")),
        edit_map(&doc.get_map("map")),
        edit_counter(&doc.get_counter("counter")),
    ));
}

/// One-way sync `from -> to` using a drawn transport encoding.
fn sync_one_way(tc: &TestCase, from: &LoroDoc, to: &LoroDoc) {
    from.commit();
    let mode = tc.draw(
        gs::sampled_from(vec![
            ExportMode::updates_owned(to.oplog_vv()),
            ExportMode::all_updates(),
            ExportMode::Snapshot,
        ])
        .print_as_debug(),
    );
    to.import(&from.export(mode).unwrap()).unwrap();
}

/// Report a drawn doc by its deep value rather than its `Debug` internals.
fn print_deep_value(doc: &LoroDoc, printer: &mut PrettyPrinter) {
    printer.text(&format!("{:?}", doc.get_deep_value()));
}

/// A doc with a random multi-peer history; peer ids start at `peer_base`.
#[hegel::composite]
fn random_doc(tc: &TestCase, peer_base: u64) -> LoroDoc {
    let n_peers = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    let docs: Vec<LoroDoc> = (0..n_peers)
        .map(|i| {
            let d = LoroDoc::new();
            d.set_peer_id(peer_base + i as u64).unwrap();
            d
        })
        .collect();
    let n_edits = tc.draw(gs::integers::<usize>().max_value(15));
    for _ in 0..n_edits {
        let i = tc.draw(gs::integers::<usize>().max_value(n_peers - 1));
        apply_random_edit(tc, &docs[i]);
    }
    for d in &docs {
        d.commit();
    }
    for d in docs.iter().skip(1) {
        let bytes = d.export(ExportMode::all_updates()).unwrap();
        docs[0].import(&bytes).unwrap();
    }
    docs.into_iter().next().unwrap()
}

#[hegel::test]
fn test_concurrent_replicas_converge(tc: TestCase) {
    let n_peers = tc.draw(gs::integers::<usize>().min_value(2).max_value(4));

    // Initialise `n_peers` docs
    let docs: Vec<LoroDoc> = (0..n_peers)
        .map(|i| {
            let d = LoroDoc::new();
            d.set_peer_id(i as u64 + 1).unwrap();
            touch_roots(&d);
            d
        })
        .collect();

    let n_actions = tc.draw(gs::integers::<usize>().min_value(1).max_value(40));
    for _ in 0..n_actions {
        let i = tc.draw(gs::integers::<usize>().max_value(n_peers - 1));
        if tc.draw(gs::weighted_booleans(0.2)) {
            // partial sync i -> j leaves replicas divergent
            let others: Vec<usize> = (0..n_peers).filter(|&j| j != i).collect();
            let j = tc.draw(gs::sampled_from(others));
            sync_one_way(&tc, &docs[i], &docs[j]);
        } else {
            apply_random_edit(&tc, &docs[i]);
        }
    }

    // Full mesh exchange in a drawn order
    let pairs: Vec<(usize, usize)> = (0..n_peers)
        .flat_map(|a| (0..n_peers).map(move |b| (a, b)))
        .filter(|(a, b)| a != b)
        .collect();
    for (a, b) in tc.draw(gs::permutations(pairs)) {
        sync_one_way(&tc, &docs[a], &docs[b]);
    }

    // Check that all docs agree
    for doc in docs.iter().skip(1) {
        assert_docs_agree(&docs[0], doc);
    }
}

#[hegel::test]
fn test_export_modes_preserve_state(tc: TestCase) {
    let doc = tc.draw(random_doc(1).print_with(print_deep_value));
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    let mode = tc.draw(
        gs::sampled_from(vec![
            ExportMode::Snapshot,
            ExportMode::shallow_snapshot(&frontiers),
            ExportMode::state_only(None),
            ExportMode::snapshot_at(&frontiers),
            ExportMode::all_updates(),
        ])
        .print_as_debug(),
    );
    let is_snapshot = matches!(mode, ExportMode::Snapshot);
    let full_history = matches!(
        mode,
        ExportMode::Snapshot | ExportMode::SnapshotAt { .. } | ExportMode::Updates { .. }
    );
    let bytes = doc.export(mode).unwrap();

    // exercise both `from_snapshot` and `import`
    let restored = if is_snapshot && tc.draw(gs::booleans()) {
        LoroDoc::from_snapshot(&bytes).unwrap()
    } else {
        let d = LoroDoc::new();
        d.import(&bytes).unwrap();
        d
    };

    // normalise empty roots on both sides
    touch_roots(&doc);
    touch_roots(&restored);
    // full-oplog modes must reproduce version info; shallow modes only the value
    if full_history {
        assert_docs_agree(&doc, &restored);
    } else {
        assert_eq!(doc.get_deep_value(), restored.get_deep_value());
    }
}

#[hegel::test]
fn test_import_is_idempotent(tc: TestCase) {
    let source = tc.draw(random_doc(1).print_with(print_deep_value));
    source.commit();
    let bytes = if tc.draw(gs::booleans()) {
        source.export(ExportMode::all_updates()).unwrap()
    } else {
        source.export(ExportMode::Snapshot).unwrap()
    };
    // target may already have disjoint-peer history
    let target = if tc.draw(gs::booleans()) {
        tc.draw(random_doc(100).print_with(print_deep_value))
    } else {
        LoroDoc::new()
    };
    target.import(&bytes).unwrap();
    // state after the first import
    let once = target.fork();
    let n_extra = tc.draw(gs::integers::<usize>().min_value(1).max_value(10));
    for _ in 0..n_extra {
        target.import(&bytes).unwrap();
    }
    assert_docs_agree(&once, &target);
}

#[hegel::test]
fn test_checkout_reproduces_recorded_states(tc: TestCase) {
    let doc = tc.draw(random_doc(1).print_with(print_deep_value));
    // Materialise roots before the first checkpoint: containers are never
    // removed from state, so earlier checkpoints must already include them.
    touch_roots(&doc);
    doc.commit();
    let mut checkpoints = vec![(doc.state_frontiers(), doc.get_deep_value())];
    let n_edits = tc.draw(gs::integers::<usize>().min_value(1).max_value(15));
    for _ in 0..n_edits {
        apply_random_edit(&tc, &doc);
        doc.commit();
        checkpoints.push((doc.state_frontiers(), doc.get_deep_value()));
    }
    let latest = doc.get_deep_value();

    let order: Vec<usize> = (0..checkpoints.len()).collect();
    for idx in tc.draw(gs::permutations(order)) {
        let (frontiers, expected) = &checkpoints[idx];
        doc.checkout(frontiers).unwrap();
        assert_eq!(*expected, doc.get_deep_value());
    }
    doc.checkout_to_latest();
    assert_eq!(latest, doc.get_deep_value());
    assert!(!doc.is_detached());
}

/// Known bug: importing a counter increment and its matching decrement in
/// separate batches drops the zero-sum diff. Any non-zero `x` reproduces it,
/// so this is a plain example test rather than a property.
#[test]
#[ignore = "https://github.com/loro-dev/loro/issues/1112"]
fn counter_zero_sum_import_batching_converges() {
    let x = 1.0;
    let origin = LoroDoc::new();
    origin.set_peer_id(1).unwrap();
    let counter = origin.get_counter("counter");
    counter.increment(x).unwrap();
    origin.commit();
    let mid_vv = origin.oplog_vv();
    counter.decrement(x).unwrap();
    origin.commit();

    let two_batches = LoroDoc::new();
    two_batches.set_peer_id(2).unwrap();
    two_batches
        .import(&origin.export(ExportMode::updates_till(&mid_vv)).unwrap())
        .unwrap();
    two_batches
        .import(&origin.export(ExportMode::updates(&mid_vv)).unwrap())
        .unwrap();

    let one_batch = LoroDoc::new();
    one_batch.set_peer_id(3).unwrap();
    one_batch
        .import(&origin.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    assert_docs_agree(&two_batches, &one_batch);
}
