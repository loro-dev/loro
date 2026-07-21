//! Tests for `LoroDoc::compare_cursors` — the tombstone-stable total order over
//! text cursors, defined for deleted (tombstoned) characters and convergent
//! across peers.
#![cfg(feature = "persistent-anchor-tracker")]

use std::cmp::Ordering;

use loro::cursor::{Cursor, Side};
use loro::{ContainerTrait, ExportMode, LoroDoc, UndoManager, ID};

/// A tiny deterministic xorshift PRNG so the property test is reproducible.
struct Rng(u64);
impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

fn doc_with_peer(peer: u64) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer).unwrap();
    doc
}

fn cursor_at(doc: &LoroDoc, index: usize, side: Side) -> Cursor {
    doc.get_text("text")
        .get_cursor(index, side)
        .expect("cursor for a present character")
}

/// Build a sentinel cursor (no id) for the root "text" container.
fn sentinel(doc: &LoroDoc, side: Side) -> Cursor {
    let container = doc.get_text("text").id();
    Cursor::new(None, container, side, 0)
}

fn sync(a: &LoroDoc, b: &LoroDoc) {
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
}

/// Assert `compare_cursors` induces a strict total order over `cursors`:
/// reflexive-as-equal, antisymmetric, and transitive.
fn assert_strict_total_order(doc: &LoroDoc, cursors: &[Cursor]) {
    // Reflexivity + antisymmetry.
    for (i, a) in cursors.iter().enumerate() {
        assert_eq!(
            doc.compare_cursors(a, a).unwrap(),
            Ordering::Equal,
            "cursor {i} must be equal to itself"
        );
        for (j, b) in cursors.iter().enumerate() {
            let ab = doc.compare_cursors(a, b).unwrap();
            let ba = doc.compare_cursors(b, a).unwrap();
            assert_eq!(
                ab,
                ba.reverse(),
                "antisymmetry violated for cursors {i} and {j}"
            );
            if i != j {
                // Distinct ids must never compare Equal.
                assert_ne!(
                    ab,
                    Ordering::Equal,
                    "distinct cursors {i},{j} compared Equal"
                );
            }
        }
    }

    // Transitivity: sorting by the comparator must yield an order with no
    // backward pair. If the relation were intransitive, some earlier element
    // would compare Greater than a later one.
    let mut sorted: Vec<usize> = (0..cursors.len()).collect();
    sorted.sort_by(|&x, &y| doc.compare_cursors(&cursors[x], &cursors[y]).unwrap());
    for a in 0..sorted.len() {
        for b in (a + 1)..sorted.len() {
            let ord = doc
                .compare_cursors(&cursors[sorted[a]], &cursors[sorted[b]])
                .unwrap();
            assert_ne!(
                ord,
                Ordering::Greater,
                "transitivity violated: sorted[{a}] > sorted[{b}]"
            );
        }
    }
}

#[test]
fn basic_order_and_reflexivity() {
    let doc = doc_with_peer(1);
    let text = doc.get_text("text");
    text.insert(0, "ab").unwrap();
    doc.commit();

    let a = cursor_at(&doc, 0, Side::Middle);
    let b = cursor_at(&doc, 1, Side::Middle);

    assert_eq!(doc.compare_cursors(&a, &b).unwrap(), Ordering::Less);
    assert_eq!(doc.compare_cursors(&b, &a).unwrap(), Ordering::Greater);
    assert_eq!(doc.compare_cursors(&a, &a).unwrap(), Ordering::Equal);
    assert_eq!(doc.compare_cursors(&b, &b).unwrap(), Ordering::Equal);
}

#[test]
fn sentinels_order_around_every_character() {
    let doc = doc_with_peer(1);
    let text = doc.get_text("text");
    text.insert(0, "abc").unwrap();
    doc.commit();

    let start = sentinel(&doc, Side::Left);
    let end = sentinel(&doc, Side::Right);
    let a = cursor_at(&doc, 0, Side::Middle);
    let c = cursor_at(&doc, 2, Side::Middle);

    // Start < any char < End.
    assert_eq!(doc.compare_cursors(&start, &a).unwrap(), Ordering::Less);
    assert_eq!(doc.compare_cursors(&a, &start).unwrap(), Ordering::Greater);
    assert_eq!(doc.compare_cursors(&c, &end).unwrap(), Ordering::Less);
    assert_eq!(doc.compare_cursors(&end, &c).unwrap(), Ordering::Greater);

    // Start < End.
    assert_eq!(doc.compare_cursors(&start, &end).unwrap(), Ordering::Less);
    assert_eq!(
        doc.compare_cursors(&end, &start).unwrap(),
        Ordering::Greater
    );

    // Sentinels compare equal to themselves.
    assert_eq!(
        doc.compare_cursors(&start, &sentinel(&doc, Side::Left))
            .unwrap(),
        Ordering::Equal
    );
    assert_eq!(
        doc.compare_cursors(&end, &sentinel(&doc, Side::Right))
            .unwrap(),
        Ordering::Equal
    );

    // A `None` + `Side::Middle` sentinel folds to End, just like `Side::Right`,
    // so the two compare Equal — this pins the classify() fold.
    let end_middle = sentinel(&doc, Side::Middle);
    assert_eq!(
        doc.compare_cursors(&end_middle, &end).unwrap(),
        Ordering::Equal
    );
    assert_eq!(
        doc.compare_cursors(&end_middle, &a).unwrap(),
        Ordering::Greater
    );
}

#[test]
fn strict_order_between_two_ids_in_one_deleted_run() {
    let doc = doc_with_peer(1);
    let text = doc.get_text("text");
    text.insert(0, "ABCDE").unwrap();
    doc.commit();

    // Capture two interior ids before the deletion merges them into one run.
    let b = cursor_at(&doc, 1, Side::Middle);
    let d = cursor_at(&doc, 3, Side::Middle);

    // Delete the whole run; both ids are now tombstoned inside one RLE span.
    text.delete(0, 5).unwrap();
    doc.commit();

    let ord = doc.compare_cursors(&b, &d).unwrap();
    assert_eq!(
        ord,
        Ordering::Less,
        "two distinct tombstoned ids in one run must order strictly, not Equal"
    );
    assert_eq!(doc.compare_cursors(&d, &b).unwrap(), Ordering::Greater);
    assert_ne!(doc.compare_cursors(&b, &d).unwrap(), Ordering::Equal);
}

#[test]
fn one_live_one_tombstoned() {
    let doc = doc_with_peer(1);
    let text = doc.get_text("text");
    text.insert(0, "ABCDE").unwrap();
    doc.commit();

    let b = cursor_at(&doc, 1, Side::Middle);
    let d = cursor_at(&doc, 3, Side::Middle);

    // Delete "BC" only: B is tombstoned, D stays live.
    text.delete(1, 2).unwrap();
    doc.commit();

    // Sanity: D is still live, B is not.
    assert!(doc.get_cursor_pos(&d).unwrap().update.is_none());
    assert!(doc.get_cursor_pos(&b).unwrap().update.is_some());

    assert_eq!(doc.compare_cursors(&b, &d).unwrap(), Ordering::Less);
    assert_eq!(doc.compare_cursors(&d, &b).unwrap(), Ordering::Greater);
}

#[test]
fn order_survives_snapshot_roundtrip_and_checkout() {
    let doc = doc_with_peer(1);
    let text = doc.get_text("text");
    text.insert(0, "ABCDE").unwrap();
    doc.commit();

    // Frontier of the all-live state, before any deletion.
    let live_frontier = doc.oplog_frontiers();

    let b = cursor_at(&doc, 1, Side::Middle);
    let d = cursor_at(&doc, 3, Side::Middle);

    // Delete "BC": b is tombstoned, d stays live.
    text.delete(1, 2).unwrap();
    doc.commit();

    let baseline = doc.compare_cursors(&b, &d).unwrap();
    assert_eq!(baseline, Ordering::Less);

    // A full-history snapshot round-trip into a fresh doc must preserve the
    // order — the tracker is rebuilt from the imported oplog every call.
    let snapshot = doc.export(ExportMode::Snapshot).unwrap();
    let restored = LoroDoc::new();
    restored.import(&snapshot).unwrap();
    assert_eq!(
        restored.compare_cursors(&b, &d).unwrap(),
        baseline,
        "snapshot round-trip changed the order"
    );

    // Checking out to an older version and back must not change the answer:
    // compare_cursors reads the oplog, not the checked-out state.
    doc.checkout(&live_frontier).unwrap();
    assert_eq!(
        doc.compare_cursors(&b, &d).unwrap(),
        baseline,
        "checkout to an older frontier changed the order"
    );
    doc.checkout_to_latest();
    assert_eq!(
        doc.compare_cursors(&b, &d).unwrap(),
        baseline,
        "checkout back to latest changed the order"
    );
}

#[test]
fn concurrent_insert_at_tombstone_seam_is_stable_and_convergent() {
    // Base doc that both peers share.
    let a = doc_with_peer(1);
    let text_a = a.get_text("text");
    text_a.insert(0, "ABCDE").unwrap();
    a.commit();

    let b = doc_with_peer(2);
    sync(&a, &b);

    // Capture an id in the run peer A is about to delete.
    let c = cursor_at(&a, 2, Side::Middle); // 'C'

    // Peer A deletes the interior run "BCD".
    text_a.delete(1, 3).unwrap();
    a.commit();

    // Peer B concurrently inserts 'X' inside that same run (before it sees A's
    // delete), at the boundary between 'B' and 'C' — so X lands immediately
    // left of C in the fugue order.
    let text_b = b.get_text("text");
    text_b.insert(2, "X").unwrap();
    b.commit();
    let x = cursor_at(&b, 2, Side::Middle); // 'X'

    // Pull B's insert into A, but do NOT push A's delete into B. Now the two
    // oplogs are divergent (B has never seen the delete; A has) yet overlapping
    // (both hold the base insert and B's insert), so both can resolve C and X.
    // On B, C is still LIVE and sits to the right of X; on A, C is a TOMBSTONE.
    // A tombstone-stable order must return the same answer on both regardless.
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    // X was inserted immediately before C, so C orders strictly after X.
    let expected = Ordering::Greater;
    assert_eq!(
        a.compare_cursors(&c, &x).unwrap(),
        expected,
        "tombstoned C must order after X on peer A"
    );
    assert_eq!(
        b.compare_cursors(&c, &x).unwrap(),
        expected,
        "live C must order after X on peer B — agreeing with A despite divergence"
    );
    assert_eq!(a.compare_cursors(&x, &c).unwrap(), expected.reverse());
    assert_eq!(b.compare_cursors(&x, &c).unwrap(), expected.reverse());

    // A third peer that imports the very same ops in a DIFFERENT order (B's
    // updates first, then A's) must derive the identical order — the real
    // convergence property: independent of arrival/merge order.
    let d = doc_with_peer(3);
    d.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    d.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(
        d.compare_cursors(&c, &x).unwrap(),
        expected,
        "arrival-order-independent order disagreed"
    );

    // Full merge: every peer now holds the same history and still agrees.
    sync(&a, &b);
    d.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(a.compare_cursors(&c, &x).unwrap(), expected);
    assert_eq!(b.compare_cursors(&c, &x).unwrap(), expected);
    assert_eq!(d.compare_cursors(&c, &x).unwrap(), expected);
}

#[test]
fn undo_resurrection_order_is_stable_and_total() {
    let doc = doc_with_peer(1);
    let mut undo = UndoManager::new(&doc);
    let text = doc.get_text("text");

    // Insert "ABC"; A and C will stay live for the whole test, only B is deleted.
    text.insert(0, "ABC").unwrap();
    doc.commit();
    let a_live = cursor_at(&doc, 0, Side::Middle); // 'A', never deleted
    let original_b = cursor_at(&doc, 1, Side::Middle); // 'B', about to be tombstoned
    let c_live = cursor_at(&doc, 2, Side::Middle); // 'C', never deleted

    text.delete(1, 1).unwrap(); // "AC"; B tombstoned between the live A and C
    doc.commit();
    undo.record_new_checkpoint().unwrap();

    // Undo resurrects B with a fresh id, back between A and C; A and C keep
    // their original ids, and the original B stays tombstoned.
    assert!(undo.undo().unwrap());
    doc.commit();
    assert_eq!(text.to_string(), "ABC");
    let resurrected_b = cursor_at(&doc, 1, Side::Middle);

    // The original and resurrected B are distinct, both resolvable, antisymmetric.
    let ob_vs_rb = doc.compare_cursors(&original_b, &resurrected_b).unwrap();
    assert_ne!(
        ob_vs_rb,
        Ordering::Equal,
        "original-tombstone and resurrected ids must be distinct in the order"
    );
    assert_eq!(
        doc.compare_cursors(&resurrected_b, &original_b).unwrap(),
        ob_vs_rb.reverse()
    );

    // Transitivity ACROSS the resurrection seam: the resurrected B sits strictly
    // between the two live neighbours.
    assert_eq!(
        doc.compare_cursors(&a_live, &resurrected_b).unwrap(),
        Ordering::Less,
        "live A must order before resurrected B"
    );
    assert_eq!(
        doc.compare_cursors(&resurrected_b, &c_live).unwrap(),
        Ordering::Less,
        "resurrected B must order before live C"
    );
    assert_eq!(
        doc.compare_cursors(&a_live, &c_live).unwrap(),
        Ordering::Less
    );

    // The tombstoned original B also has a defined position between A and C.
    assert_eq!(
        doc.compare_cursors(&a_live, &original_b).unwrap(),
        Ordering::Less
    );
    assert_eq!(
        doc.compare_cursors(&original_b, &c_live).unwrap(),
        Ordering::Less
    );

    // The whole quadruple is a strict total order.
    assert_strict_total_order(&doc, &[a_live, c_live, original_b, resurrected_b]);
}

#[test]
fn non_panicking_guards_return_err() {
    let doc = doc_with_peer(1);
    let text = doc.get_text("text");
    text.insert(0, "ABCDE").unwrap(); // counters 0..5 under op id (1, 0)
    doc.commit();
    text.delete(1, 2).unwrap(); // delete "BC"; the delete op id is (1, 5)
    doc.commit();

    let container = text.id();
    let live = cursor_at(&doc, 0, Side::Middle); // 'A', a live real id

    // A foreign/unknown id returns Err on the resolution path, never a panic.
    let foreign = Cursor::new(Some(ID::new(9999, 7)), container.clone(), Side::Middle, 0);
    assert!(doc.compare_cursors(&foreign, &live).is_err());
    assert!(doc.compare_cursors(&live, &foreign).is_err());
    // Foreign vs foreign also errs cleanly.
    let foreign2 = Cursor::new(Some(ID::new(8888, 3)), container.clone(), Side::Middle, 0);
    assert!(doc.compare_cursors(&foreign, &foreign2).is_err());

    // An id that lands in a deletion fragment (the delete op id itself) hits the
    // `Cursor::Delete => unreachable!()` path made reachable by a user cursor;
    // it must return Err, not panic.
    let delete_op_id = Cursor::new(Some(ID::new(1, 5)), container.clone(), Side::Middle, 0);
    assert!(doc.compare_cursors(&delete_op_id, &live).is_err());

    // Cross-container comparison returns Err rather than a meaningless order.
    let other = doc.get_text("other");
    other.insert(0, "z").unwrap();
    doc.commit();
    let other_cursor = other.get_cursor(0, Side::Middle).unwrap();
    assert!(doc.compare_cursors(&live, &other_cursor).is_err());

    // Sentinels short-circuit: Start orders below and End above everything in
    // their container, without resolving the counterpart id, so they never
    // panic or err even against a foreign id.
    let start = sentinel(&doc, Side::Left);
    let end = sentinel(&doc, Side::Right);
    assert_eq!(
        doc.compare_cursors(&start, &foreign).unwrap(),
        Ordering::Less
    );
    assert_eq!(
        doc.compare_cursors(&end, &foreign).unwrap(),
        Ordering::Greater
    );
}

#[test]
fn live_order_agrees_with_get_cursor_pos() {
    let doc = doc_with_peer(1);
    let text = doc.get_text("text");
    text.insert(0, "hello world").unwrap();
    doc.commit();

    // Interleave a deletion so tombstones sit between live chars.
    text.delete(5, 1).unwrap(); // drop the space
    doc.commit();

    let mut live: Vec<Cursor> = Vec::new();
    let len = text.len_unicode();
    for i in 0..len {
        let c = cursor_at(&doc, i, Side::Middle);
        // Only keep ids that are currently live.
        if doc.get_cursor_pos(&c).unwrap().update.is_none() {
            live.push(c);
        }
    }
    assert!(live.len() >= 5);

    // compare_cursors must agree with the live positions.
    for i in 0..live.len() {
        for j in 0..live.len() {
            let pos_i = doc.get_cursor_pos(&live[i]).unwrap().current.pos;
            let pos_j = doc.get_cursor_pos(&live[j]).unwrap().current.pos;
            let expected = pos_i.cmp(&pos_j);
            let got = doc.compare_cursors(&live[i], &live[j]).unwrap();
            assert_eq!(got, expected, "live order disagreed at {i},{j}");
        }
    }
}

#[test]
fn determinism_repeated_calls_are_identical() {
    let doc = doc_with_peer(1);
    let text = doc.get_text("text");
    text.insert(0, "ABCDE").unwrap();
    doc.commit();
    let b = cursor_at(&doc, 1, Side::Middle);
    let d = cursor_at(&doc, 3, Side::Middle);
    text.delete(0, 5).unwrap();
    doc.commit();

    let first = doc.compare_cursors(&b, &d).unwrap();
    for _ in 0..10 {
        assert_eq!(doc.compare_cursors(&b, &d).unwrap(), first);
    }
}

#[test]
fn property_total_order_over_multi_peer_script() {
    let mut rng = Rng(0x9E3779B97F4A7C15);

    let a = doc_with_peer(1);
    let b = doc_with_peer(2);
    let c = doc_with_peer(3);
    let docs = [&a, &b, &c];

    // Seed some shared content so early deletes have something to bite.
    a.get_text("text").insert(0, "seed-content").unwrap();
    a.commit();
    sync(&a, &b);
    sync(&a, &c);
    sync(&b, &c);

    let mut captured: Vec<Cursor> = Vec::new();

    for round in 0..40 {
        let doc = docs[rng.below(docs.len())];
        let text = doc.get_text("text");
        let len = text.len_unicode();

        // Bias toward inserts so the document keeps growing.
        if len == 0 || rng.below(3) != 0 {
            let pos = if len == 0 { 0 } else { rng.below(len + 1) };
            let ch = (b'a' + (round % 26) as u8) as char;
            text.insert(pos, &ch.to_string()).unwrap();
            doc.commit();
            // Capture the freshly inserted id.
            captured.push(cursor_at(doc, pos, Side::Middle));
        } else {
            let start = rng.below(len);
            let del_len = 1 + rng.below((len - start).min(3));
            // Capture a couple of ids from the run before deleting them.
            captured.push(cursor_at(doc, start, Side::Middle));
            text.delete(start, del_len).unwrap();
            doc.commit();
        }

        // Occasionally sync a random pair of peers.
        if rng.below(2) == 0 {
            let i = rng.below(docs.len());
            let j = (i + 1 + rng.below(docs.len() - 1)) % docs.len();
            sync(docs[i], docs[j]);
        }
    }

    // Full merge so every doc holds the same history.
    sync(&a, &b);
    sync(&a, &c);
    sync(&b, &c);
    sync(&a, &b);

    // Deduplicate cursors by their id so identical captures don't force a
    // spurious "distinct ids" failure.
    captured.sort_by(|x, y| format!("{:?}", x.id).cmp(&format!("{:?}", y.id)));
    captured.dedup_by(|x, y| x.id == y.id);
    assert!(captured.len() >= 10, "expected a decent pool of ids");

    // The order is a strict total order on every peer, and all peers agree.
    assert_strict_total_order(&a, &captured);
    for (i, x) in captured.iter().enumerate() {
        for (j, y) in captured.iter().enumerate() {
            let oa = a.compare_cursors(x, y).unwrap();
            let ob = b.compare_cursors(x, y).unwrap();
            let oc = c.compare_cursors(x, y).unwrap();
            assert_eq!(oa, ob, "peers 1 and 2 disagree at {i},{j}");
            assert_eq!(oa, oc, "peers 1 and 3 disagree at {i},{j}");
        }
    }

    // Live-id order agrees with get_cursor_pos on peer A.
    let live: Vec<Cursor> = captured
        .iter()
        .filter(|c| {
            a.get_cursor_pos(c)
                .map(|r| r.update.is_none())
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    for x in &live {
        for y in &live {
            let px = a.get_cursor_pos(x).unwrap().current.pos;
            let py = a.get_cursor_pos(y).unwrap().current.pos;
            assert_eq!(a.compare_cursors(x, y).unwrap(), px.cmp(&py));
        }
    }
}

#[test]
fn cached_path_matches_genesis_over_fuzz_corpus() {
    let mut rng = Rng(0xD1B54A32D192ED03);

    let a = doc_with_peer(1);
    let b = doc_with_peer(2);
    let c = doc_with_peer(3);
    let docs = [&a, &b, &c];

    a.get_text("text").insert(0, "seed-content").unwrap();
    a.commit();
    sync(&a, &b);
    sync(&a, &c);
    sync(&b, &c);

    // Compare full results, including which `Err` variant, so the cached path's
    // unresolved-id taxonomy is checked against the oracle, not only the `Ok`
    // orderings. `CannotFindRelativePosition` is not `PartialEq`, so fold each
    // result to a form that is.
    fn norm(
        r: Result<Ordering, loro::cursor::CannotFindRelativePosition>,
    ) -> Result<Ordering, String> {
        r.map_err(|e| format!("{e:?}"))
    }

    let mut captured: Vec<Cursor> = Vec::new();

    for round in 0..60 {
        let doc = docs[rng.below(docs.len())];
        let text = doc.get_text("text");
        let len = text.len_unicode();

        if len == 0 || rng.below(3) != 0 {
            let pos = if len == 0 { 0 } else { rng.below(len + 1) };
            let ch = (b'a' + (round % 26) as u8) as char;
            text.insert(pos, &ch.to_string()).unwrap();
            doc.commit();
            captured.push(cursor_at(doc, pos, Side::Middle));
        } else {
            let start = rng.below(len);
            let del_len = 1 + rng.below((len - start).min(3));
            captured.push(cursor_at(doc, start, Side::Middle));
            text.delete(start, del_len).unwrap();
            doc.commit();
        }

        if rng.below(2) == 0 {
            let i = rng.below(docs.len());
            let j = (i + 1 + rng.below(docs.len() - 1)) % docs.len();
            sync(docs[i], docs[j]);
        }

        // Query mid-stream so the cached tracker advances incrementally across
        // the whole script rather than building once at the end, and check each
        // advance against a fresh genesis rebuild.
        if captured.len() >= 2 {
            let x = &captured[captured.len() - 1];
            let y = &captured[captured.len() - 2];
            assert_eq!(
                norm(doc.compare_cursors(x, y)),
                norm(doc.__compare_cursors_via_genesis(x, y)),
                "cached vs genesis disagreed mid-stream"
            );
        }
    }

    // Full merge so every doc holds the same history.
    sync(&a, &b);
    sync(&a, &c);
    sync(&b, &c);
    sync(&a, &b);

    captured.sort_by(|x, y| format!("{:?}", x.id).cmp(&format!("{:?}", y.id)));
    captured.dedup_by(|x, y| x.id == y.id);
    assert!(captured.len() >= 10, "expected a decent pool of ids");

    // Add an unresolvable (foreign) id so the `Err` taxonomy is differentially
    // checked: both paths must reject it identically.
    let container = a.get_text("text").id();
    captured.push(Cursor::new(
        Some(ID::new(1 << 40, 0)),
        container,
        Side::Middle,
        0,
    ));

    // The cached warm-tracker path must agree with a fresh genesis rebuild for
    // every pair on every peer, including the error taxonomy.
    for doc in docs {
        for x in &captured {
            for y in &captured {
                assert_eq!(
                    norm(doc.compare_cursors(x, y)),
                    norm(doc.__compare_cursors_via_genesis(x, y)),
                    "cached path diverged from the genesis rebuild"
                );
            }
        }
    }
}

#[test]
fn multi_container_document_orders_correctly() {
    // A document with a text container beside a map container: the map ops
    // advance the document version without touching the text tracker, which the
    // version-gated advance must handle without misfiring.
    let doc = doc_with_peer(1);
    let text = doc.get_text("text");
    let map = doc.get_map("m");

    text.insert(0, "ABCDE").unwrap();
    doc.commit();
    map.insert("k", 1).unwrap();
    doc.commit();

    let b = cursor_at(&doc, 1, Side::Middle);
    let d = cursor_at(&doc, 3, Side::Middle);

    text.delete(0, 5).unwrap();
    doc.commit();

    // More map ops: the document head moves forward with no new text ops.
    map.insert("k2", 2).unwrap();
    doc.commit();
    map.insert("k3", 3).unwrap();
    doc.commit();

    assert_eq!(doc.compare_cursors(&b, &d).unwrap(), Ordering::Less);
    assert_eq!(doc.compare_cursors(&d, &b).unwrap(), Ordering::Greater);
    assert_eq!(
        doc.compare_cursors(&b, &d).ok(),
        doc.__compare_cursors_via_genesis(&b, &d).ok(),
        "cached path diverged from genesis on a multi-container doc"
    );

    // Repeated queries at the stable head remain identical.
    let first = doc.compare_cursors(&b, &d).unwrap();
    for _ in 0..5 {
        assert_eq!(doc.compare_cursors(&b, &d).unwrap(), first);
    }
}

#[test]
fn query_after_insert_flushes_pending_edit_and_survives_checkout() {
    // Auto-commit flushes the open transaction through `with_barrier` before the
    // query reads the oplog, so a just-typed char resolves without a manual
    // commit.
    let doc = doc_with_peer(1);
    let text = doc.get_text("text");
    text.insert(0, "AB").unwrap();
    // Deliberately no doc.commit() here.
    let a = cursor_at(&doc, 0, Side::Middle);
    let b = cursor_at(&doc, 1, Side::Middle);
    assert_eq!(doc.compare_cursors(&a, &b).unwrap(), Ordering::Less);

    // Insert more, remember the head, then insert once more.
    text.insert(2, "C").unwrap();
    doc.commit();
    let mid = doc.oplog_frontiers();
    text.insert(3, "D").unwrap();
    doc.commit();

    // Checked out to an earlier version, the document is detached, yet
    // `compare_cursors` reads the oplog head and returns the same answer.
    doc.checkout(&mid).unwrap();
    let c = cursor_at(&doc, 2, Side::Middle); // 'C', live in the checked-out state
    assert_eq!(doc.compare_cursors(&a, &c).unwrap(), Ordering::Less);
    assert_eq!(
        doc.compare_cursors(&a, &c).ok(),
        doc.__compare_cursors_via_genesis(&a, &c).ok()
    );
    doc.checkout_to_latest();
    assert_eq!(doc.compare_cursors(&a, &c).unwrap(), Ordering::Less);
}

// ----------------------------------------------------------------------------
// `get_cursor_pos` fast path — a tombstoned-text query answered off the warm
// settled tracker (O(log n)) instead of the per-call genesis replay. The
// overriding constraint is byte-identical observable behavior: the fast path
// must return the same `PosQueryResult` (including the `update` field) as the
// original slow path, only faster.
// ----------------------------------------------------------------------------

/// Fold a `get_cursor_pos` result into a fully comparable form: the current
/// position, its side, and the whole `update` cursor (so `update.is_some()` and
/// its contents are both checked), with the error variant folded to a string
/// (`CannotFindRelativePosition` is not `PartialEq`).
fn norm_pos(
    r: Result<loro::cursor::PosQueryResult, loro::cursor::CannotFindRelativePosition>,
) -> Result<(usize, Side, Option<Cursor>), String> {
    r.map(|p| (p.current.pos, p.current.side, p.update))
        .map_err(|e| format!("{e:?}"))
}

#[test]
fn tombstoned_cursor_pos_fast_path_matches_genesis_over_fuzz_corpus() {
    // The crux end-to-end differential: a tombstoned `get_cursor_pos` served by
    // the warm-tracker fast path must return the identical full `PosQueryResult`
    // (incl. `update.is_some()` and its contents) as the original genesis replay,
    // across an insert/delete/merge script on multiple peers.
    let mut rng = Rng(0x5EED_1234_ABCD_00FF);

    let a = doc_with_peer(1);
    let b = doc_with_peer(2);
    let c = doc_with_peer(3);
    let docs = [&a, &b, &c];

    a.get_text("text").insert(0, "seed-content").unwrap();
    a.commit();
    sync(&a, &b);
    sync(&a, &c);
    sync(&b, &c);

    let mut captured: Vec<Cursor> = Vec::new();

    for round in 0..60 {
        let doc = docs[rng.below(docs.len())];
        let text = doc.get_text("text");
        let len = text.len_unicode();

        if len == 0 || rng.below(3) != 0 {
            let pos = if len == 0 { 0 } else { rng.below(len + 1) };
            let ch = (b'a' + (round % 26) as u8) as char;
            text.insert(pos, &ch.to_string()).unwrap();
            doc.commit();
            captured.push(cursor_at(doc, pos, Side::Middle));
        } else {
            let start = rng.below(len);
            let del_len = 1 + rng.below((len - start).min(3));
            for i in start..(start + del_len) {
                captured.push(cursor_at(doc, i, Side::Middle));
            }
            text.delete(start, del_len).unwrap();
            doc.commit();
        }

        if rng.below(2) == 0 {
            let i = rng.below(docs.len());
            let j = (i + 1 + rng.below(docs.len() - 1)) % docs.len();
            sync(docs[i], docs[j]);
        }

        // Query mid-stream on the just-mutated doc so the warm tracker advances
        // incrementally and each advance is checked against a fresh replay.
        if let Some(cur) = captured.last() {
            assert_eq!(
                norm_pos(doc.get_cursor_pos(cur)),
                norm_pos(doc.__get_cursor_pos_via_genesis(cur)),
                "fast path diverged from genesis mid-stream"
            );
        }
    }

    // Full merge so every doc holds the same history.
    sync(&a, &b);
    sync(&a, &c);
    sync(&b, &c);
    sync(&a, &b);

    captured.sort_by(|x, y| format!("{:?}", x.id).cmp(&format!("{:?}", y.id)));
    captured.dedup_by(|x, y| x.id == y.id);
    assert!(captured.len() >= 10, "expected a decent pool of ids");

    // On every peer, the fast path must agree with the genesis replay for every
    // captured id, and the corpus must actually exercise tombstoned ids (the
    // ones whose answer carries an `update`).
    let mut saw_tombstoned = false;
    for doc in docs {
        for cur in &captured {
            let fast = doc.get_cursor_pos(cur);
            let genesis = doc.__get_cursor_pos_via_genesis(cur);
            assert_eq!(
                norm_pos(fast.clone()),
                norm_pos(genesis),
                "fast path diverged from the genesis replay"
            );
            if let Ok(p) = fast {
                if p.update.is_some() {
                    saw_tombstoned = true;
                }
            }
        }
    }
    assert!(
        saw_tombstoned,
        "corpus must exercise at least one tombstoned get_cursor_pos"
    );
}

#[test]
fn tombstoned_cursor_pos_full_result_matches_genesis_on_delete_script() {
    // A hand-built script pinning the full `PosQueryResult` for a tombstone at a
    // seam between live characters: both the seam index and the `update` cursor
    // must equal the genesis replay's.
    let doc = doc_with_peer(1);
    let text = doc.get_text("text");
    text.insert(0, "ABCDEFG").unwrap();
    doc.commit();

    let b = cursor_at(&doc, 1, Side::Middle); // 'B'
    let e = cursor_at(&doc, 4, Side::Middle); // 'E'

    // Delete "BCD" so B sits inside a tombstoned run, then delete "E" too.
    text.delete(1, 3).unwrap();
    doc.commit();
    text.delete(1, 1).unwrap(); // deletes 'E' (now at index 1: "AEFG" -> "AFG")
    doc.commit();

    for cur in [&b, &e] {
        let fast = doc.get_cursor_pos(cur);
        // A tombstoned target carries an `update`.
        assert!(
            fast.as_ref().unwrap().update.is_some(),
            "expected a tombstoned target to carry an update cursor"
        );
        assert_eq!(
            norm_pos(fast),
            norm_pos(doc.__get_cursor_pos_via_genesis(cur)),
            "full PosQueryResult diverged from genesis for a tombstoned id"
        );
    }
}

#[test]
fn detached_doc_cursor_pos_is_unchanged() {
    // A char that is live at the oplog head but absent in a checked-out earlier
    // version must still resolve exactly as the original slow path did. The fast
    // path is gated on `!is_detached()`, so a detached doc always takes the slow
    // path; the answer must match the genesis replay.
    let doc = doc_with_peer(1);
    let text = doc.get_text("text");
    text.insert(0, "A").unwrap();
    doc.commit();
    let early = doc.oplog_frontiers(); // only 'A' exists here

    text.insert(1, "B").unwrap();
    doc.commit();
    let b = cursor_at(&doc, 1, Side::Middle); // 'B', live at head

    // Detach to the version before B existed: B is absent from the checked-out
    // state but live at the oplog head.
    doc.checkout(&early).unwrap();
    assert!(doc.is_detached());

    let result = doc.get_cursor_pos(&b);
    assert!(
        matches!(
            result,
            Err(loro::cursor::CannotFindRelativePosition::IdNotFound)
        ),
        "detached query for a not-yet-existing char must be IdNotFound, got {result:?}"
    );
    // And it agrees with the genesis replay.
    assert_eq!(
        norm_pos(doc.get_cursor_pos(&b)),
        norm_pos(doc.__get_cursor_pos_via_genesis(&b)),
    );
}

#[test]
fn shallow_doc_tombstone_falls_back_to_slow_path() {
    // On a shallow document the fast path is skipped (`!oplog.is_shallow()`),
    // because pre-root tombstones are not indexed by the warm tracker. A
    // tombstoned query must resolve via the untouched slow path, byte-identical
    // to a fresh genesis replay on the same shallow doc.
    let doc = doc_with_peer(1);
    let text = doc.get_text("text");
    text.insert(0, "ABCDEFGH").unwrap();
    doc.commit();
    let b = cursor_at(&doc, 1, Side::Middle); // 'B'
    text.delete(1, 1).unwrap(); // tombstone 'B'
    doc.commit();
    // Add more live history after the tombstone so the shallow root retains it.
    text.insert(text.len_unicode(), "IJK").unwrap();
    doc.commit();

    // Export a shallow snapshot at the head and import into a fresh doc.
    let frontiers = doc.oplog_frontiers();
    let shallow_bytes = doc
        .export(ExportMode::shallow_snapshot(&frontiers))
        .unwrap();
    let shallow = LoroDoc::new();
    shallow.import(&shallow_bytes).unwrap();
    assert!(shallow.is_shallow(), "imported doc must be shallow");

    // On the shallow doc the fast path is skipped for every query, so the warm
    // path and the genesis replay must return the identical result — whatever it
    // is (a resolved seam or a HistoryCleared/IdNotFound), they agree and neither
    // panics.
    assert_eq!(
        norm_pos(shallow.get_cursor_pos(&b)),
        norm_pos(shallow.__get_cursor_pos_via_genesis(&b)),
        "shallow-doc tombstone diverged between the warm path and the replay"
    );
}

#[test]
fn detached_tombstoned_at_head_query_matches_genesis() {
    // A char TOMBSTONED at the oplog head, queried while the doc is detached to a
    // version where it is absent, must resolve exactly as the genesis slow path.
    //
    // Unlike `detached_doc_cursor_pos_is_unchanged` (which queries a live-at-head
    // id the tombstone-only filter drops before the `!is_detached()` gate can
    // matter), this reaches the fast-path branch. The gate skips the warm tracker
    // here; removing it is caught in debug because the debug length guard would
    // then read DocState at the checked-out version — whose live length (4)
    // differs from the head tracker's (6) — and fire.
    let doc = doc_with_peer(1);
    let text = doc.get_text("text");
    text.insert(0, "ABCDE").unwrap();
    doc.commit();
    let c = cursor_at(&doc, 2, Side::Middle); // 'C'

    text.delete(2, 1).unwrap(); // tombstone 'C' -> "ABDE" (4 live)
    doc.commit();
    let after_del = doc.oplog_frontiers();

    text.insert(0, "XY").unwrap(); // head "XYABDE" (6 live)
    doc.commit();

    // Detach to just after C's deletion: C is absent there and the checked-out
    // live length (4) differs from head (6).
    doc.checkout(&after_del).unwrap();
    assert!(doc.is_detached());

    let fast = doc.get_cursor_pos(&c);
    assert!(
        fast.as_ref().unwrap().update.is_some(),
        "a tombstoned target must carry an update cursor"
    );
    assert_eq!(
        norm_pos(fast),
        norm_pos(doc.__get_cursor_pos_via_genesis(&c)),
        "detached tombstone query diverged from the genesis replay"
    );
}

#[test]
fn style_anchor_tombstone_fast_path_matches_genesis() {
    // Style marks insert style-anchor entities into the rope; when the marked
    // span is deleted those anchors are tombstoned and counted in the seam rank.
    // Nothing else covers them, so pin the fast path against genesis for cursors
    // inside a deleted, previously-marked run.
    let doc = doc_with_peer(1);
    let text = doc.get_text("text");
    text.insert(0, "abcdefgh").unwrap();
    doc.commit();
    text.mark(2..6, "bold", true).unwrap(); // style anchors around the c..f run
    doc.commit();

    // Capture ids inside the marked run before deleting it.
    let c = cursor_at(&doc, 2, Side::Middle);
    let e = cursor_at(&doc, 4, Side::Middle);

    text.delete(2, 4).unwrap(); // tombstone the marked run
    doc.commit();

    for cur in [&c, &e] {
        let fast = doc.get_cursor_pos(cur);
        assert!(
            fast.as_ref().unwrap().update.is_some(),
            "a tombstoned marked char must carry an update cursor"
        );
        assert_eq!(
            norm_pos(fast),
            norm_pos(doc.__get_cursor_pos_via_genesis(cur)),
            "style-anchor tombstone diverged between the fast path and genesis"
        );
    }
}

#[test]
fn undo_resurrected_then_redeleted_fast_path_matches_genesis() {
    // A char tombstoned, resurrected with a fresh id via undo, then re-deleted:
    // both the original and resurrected ids are tombstoned at head. The fast path
    // must resolve each identically to the genesis replay. The fuzz corpus never
    // exercises undo resurrection, so pin it here.
    let doc = doc_with_peer(1);
    let mut undo = UndoManager::new(&doc);
    let text = doc.get_text("text");

    text.insert(0, "ABC").unwrap();
    doc.commit();
    let original_b = cursor_at(&doc, 1, Side::Middle); // 'B'

    text.delete(1, 1).unwrap(); // tombstone B -> "AC"
    doc.commit();
    undo.record_new_checkpoint().unwrap();

    // Undo resurrects B with a fresh id between A and C.
    assert!(undo.undo().unwrap());
    doc.commit();
    assert_eq!(text.to_string(), "ABC");
    let resurrected_b = cursor_at(&doc, 1, Side::Middle);

    // Re-delete the resurrected B: now both B ids are tombstoned at head.
    text.delete(1, 1).unwrap();
    doc.commit();

    for cur in [&original_b, &resurrected_b] {
        let fast = doc.get_cursor_pos(cur);
        assert!(
            fast.as_ref().unwrap().update.is_some(),
            "both the original and resurrected B must be tombstoned at head"
        );
        assert_eq!(
            norm_pos(fast),
            norm_pos(doc.__get_cursor_pos_via_genesis(cur)),
            "undo-resurrection tombstone diverged between the fast path and genesis"
        );
    }
}

#[test]
fn concurrent_insert_at_tombstone_seam_cursor_pos_matches_genesis() {
    // A deterministic mirror of `concurrent_insert_at_tombstone_seam_...` but for
    // `get_cursor_pos`: a concurrent insert splits a run that one peer then
    // deletes, so the tombstone seam has a live neighbour spliced into it. The
    // fuzz corpus's splits are probabilistic; this asserts the exact shape.
    let a = doc_with_peer(1);
    let text_a = a.get_text("text");
    text_a.insert(0, "ABCDE").unwrap();
    a.commit();

    let b = doc_with_peer(2);
    sync(&a, &b);

    let c = cursor_at(&a, 2, Side::Middle); // 'C'

    text_a.delete(1, 3).unwrap(); // A deletes "BCD"
    a.commit();

    let text_b = b.get_text("text");
    text_b.insert(2, "X").unwrap(); // B inserts X at the B/C boundary (before it sees A's delete)
    b.commit();

    // Pull B's insert into A: on A, C is tombstoned and X is spliced into the
    // seam. The fast path must still match the genesis replay.
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let fast = a.get_cursor_pos(&c);
    assert!(
        fast.as_ref().unwrap().update.is_some(),
        "C must be tombstoned on peer A"
    );
    assert_eq!(
        norm_pos(fast),
        norm_pos(a.__get_cursor_pos_via_genesis(&c)),
        "concurrent-split tombstone diverged between the fast path and genesis on A"
    );
}
