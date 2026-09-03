//! Black-box convergence tests for concurrent imports.
//!
//! With a multi-head critical replay base, the richtext/list diff
//! calculators serve concurrent imports from a tracker seeded at the last
//! synced version instead of rebuilding from the whole history. These tests
//! pin that incremental path to the results of a fresh replica (whose
//! calculator replays everything from scratch), across sync patterns,
//! checkouts, styles, container mixes, and cursor queries.

use loro::{ExportMode, LoroDoc, LoroText, ToJson};

fn lcg(seed: &mut u64) -> f64 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*seed >> 11) as f64) / ((1u64 << 53) as f64)
}

fn below(seed: &mut u64, n: usize) -> usize {
    (lcg(seed) * n as f64) as usize
}

/// Two peers editing concurrently and syncing every round must converge on
/// every round (each import after the first uses the incremental path).
#[test]
fn ping_pong_converges_every_round() {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1).unwrap();
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2).unwrap();
    let text_a = doc_a.get_text("t");
    let text_b = doc_b.get_text("t");
    text_a
        .insert(0, "base text for the ping pong test")
        .unwrap();
    doc_a.commit();
    doc_b
        .import(&doc_a.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let mut seed = 42u64;
    for round in 0..60 {
        for _ in 0..5 {
            let pa = below(&mut seed, text_a.len_unicode() + 1);
            text_a.insert(pa, "x").unwrap();
            let pb = below(&mut seed, text_b.len_unicode() + 1);
            text_b.insert(pb, "y").unwrap();
            if round % 3 == 0 && text_b.len_unicode() > 10 {
                let pd = below(&mut seed, text_b.len_unicode() - 2);
                text_b.delete(pd, 1).unwrap();
            }
        }
        doc_a.commit();
        doc_b.commit();
        let ua = doc_a
            .export(ExportMode::updates(&doc_b.oplog_vv()))
            .unwrap();
        let ub = doc_b
            .export(ExportMode::updates(&doc_a.oplog_vv()))
            .unwrap();
        doc_b.import(&ua).unwrap();
        doc_a.import(&ub).unwrap();
        assert_eq!(
            text_a.to_string(),
            text_b.to_string(),
            "diverged at round {round}"
        );
    }
}

/// The incrementally maintained state must match a replica that imports the
/// full history from scratch (whose calculator takes the rebuild path).
#[test]
fn concurrent_import_matches_fresh_replica() {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1).unwrap();
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2).unwrap();
    let text_a = doc_a.get_text("t");
    let text_b = doc_b.get_text("t");
    text_a.insert(0, "0123456789").unwrap();
    doc_a.commit();
    doc_b
        .import(&doc_a.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let mut seed = 7u64;
    for _ in 0..40 {
        for _ in 0..4 {
            let pa = below(&mut seed, text_a.len_unicode() + 1);
            text_a.insert(pa, "a").unwrap();
            let pb = below(&mut seed, text_b.len_unicode() + 1);
            text_b.insert(pb, "b").unwrap();
        }
        doc_a.commit();
        doc_b.commit();
        let ua = doc_a
            .export(ExportMode::updates(&doc_b.oplog_vv()))
            .unwrap();
        let ub = doc_b
            .export(ExportMode::updates(&doc_a.oplog_vv()))
            .unwrap();
        doc_b.import(&ua).unwrap();
        doc_a.import(&ub).unwrap();
    }

    let fresh = LoroDoc::new();
    fresh
        .import(&doc_a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(fresh.get_text("t").to_string(), text_a.to_string());
}

/// Checkout (retreat) through a concurrent history must agree between the
/// incremental tracker and a fresh replica's rebuilt tracker.
#[test]
fn rollback_after_concurrent_imports_matches_fresh_replica() {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1).unwrap();
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2).unwrap();
    let text_a = doc_a.get_text("t");
    let text_b = doc_b.get_text("t");
    text_a.insert(0, "rollback base").unwrap();
    doc_a.commit();
    doc_b
        .import(&doc_a.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let mut seed = 9u64;
    let mut checkpoints = vec![];
    for round in 0..30 {
        for _ in 0..3 {
            let pa = below(&mut seed, text_a.len_unicode() + 1);
            text_a.insert(pa, "a").unwrap();
            let pb = below(&mut seed, text_b.len_unicode() + 1);
            text_b.insert(pb, "b").unwrap();
        }
        doc_a.commit();
        doc_b.commit();
        let ua = doc_a
            .export(ExportMode::updates(&doc_b.oplog_vv()))
            .unwrap();
        let ub = doc_b
            .export(ExportMode::updates(&doc_a.oplog_vv()))
            .unwrap();
        doc_b.import(&ua).unwrap();
        doc_a.import(&ub).unwrap();
        if round % 10 == 0 {
            checkpoints.push(doc_a.state_frontiers());
        }
    }

    let fresh = LoroDoc::new();
    fresh
        .import(&doc_a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let fresh_text = fresh.get_text("t");
    for f in checkpoints.iter().rev() {
        doc_a.checkout(f).unwrap();
        fresh.checkout(f).unwrap();
        assert_eq!(text_a.to_string(), fresh_text.to_string());
    }
    doc_a.checkout_to_latest();
    fresh.checkout_to_latest();
    assert_eq!(text_a.to_string(), fresh_text.to_string());
}

/// Concurrent styling plus edits: the styles vec kept alongside the tracker
/// must stay consistent on the incremental path.
#[test]
fn concurrent_styles_converge() {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1).unwrap();
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2).unwrap();
    let text_a = doc_a.get_text("t");
    let text_b = doc_b.get_text("t");
    text_a.insert(0, "hello styled world, hello again").unwrap();
    doc_a.commit();
    doc_b
        .import(&doc_a.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let mut seed = 11u64;
    for round in 0..20 {
        let la = text_a.len_unicode();
        let start_a = below(&mut seed, la - 2);
        let end_a = start_a + 1 + below(&mut seed, la - start_a - 1);
        text_a.mark(start_a..end_a, "bold", true).unwrap();
        let pa = below(&mut seed, text_a.len_unicode() + 1);
        text_a.insert(pa, "A").unwrap();

        let lb = text_b.len_unicode();
        let start_b = below(&mut seed, lb - 2);
        let end_b = start_b + 1 + below(&mut seed, lb - start_b - 1);
        text_b
            .mark(start_b..end_b, "comment", format!("r{round}"))
            .unwrap();
        let pb = below(&mut seed, text_b.len_unicode() + 1);
        text_b.insert(pb, "B").unwrap();

        doc_a.commit();
        doc_b.commit();
        let ua = doc_a
            .export(ExportMode::updates(&doc_b.oplog_vv()))
            .unwrap();
        let ub = doc_b
            .export(ExportMode::updates(&doc_a.oplog_vv()))
            .unwrap();
        doc_b.import(&ua).unwrap();
        doc_a.import(&ub).unwrap();
        assert_eq!(
            text_a.get_richtext_value(),
            text_b.get_richtext_value(),
            "richtext diverged at round {round}"
        );
    }

    let fresh = LoroDoc::new();
    fresh
        .import(&doc_a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(
        fresh.get_text("t").get_richtext_value(),
        text_a.get_richtext_value()
    );
}

/// Text nested inside a map + a concurrent map edit in the SAME round, so
/// the affected-container set mixes tracker-based and history-cache-based
/// calculators in one conservative diff.
#[test]
fn nested_text_with_concurrent_map_edits() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();

    let ma = a.get_map("m");
    ma.insert_container("t", LoroText::new()).unwrap();
    a.get_map("m").insert("k", 0).unwrap();
    a.commit();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let ta = a.get_map("m").get("t").unwrap().into_container().unwrap();
    let ta: LoroText = ta.into_text().unwrap();
    let tb = b.get_map("m").get("t").unwrap().into_container().unwrap();
    let tb: LoroText = tb.into_text().unwrap();
    ta.insert(0, "seed content here").unwrap();
    a.commit();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let mut seed = 3u64;
    for round in 0..30 {
        for _ in 0..3 {
            let pa = below(&mut seed, ta.len_unicode() + 1);
            ta.insert(pa, "a").unwrap();
            let pb = below(&mut seed, tb.len_unicode() + 1);
            tb.insert(pb, "b").unwrap();
        }
        // concurrent map edits in the same round -> mixed changed containers
        a.get_map("m").insert("ka", round).unwrap();
        b.get_map("m").insert("kb", round).unwrap();
        a.commit();
        b.commit();
        let ua = a.export(ExportMode::updates(&b.oplog_vv())).unwrap();
        let ub = b.export(ExportMode::updates(&a.oplog_vv())).unwrap();
        b.import(&ua).unwrap();
        a.import(&ub).unwrap();
        assert_eq!(ta.to_string(), tb.to_string(), "diverged round {round}");
    }

    let fresh = LoroDoc::new();
    fresh
        .import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let ft = fresh
        .get_map("m")
        .get("t")
        .unwrap()
        .into_container()
        .unwrap()
        .into_text()
        .unwrap();
    assert_eq!(ft.to_string(), ta.to_string());
    assert_eq!(
        fresh.get_deep_value().to_json(),
        a.get_deep_value().to_json()
    );
}

/// A checkout BEFORE any import exercises the persistent (checkout)
/// calculator first; the per-import calculators later run with critical
/// bases of their own. The two must agree at every checkpoint.
#[test]
fn checkout_before_first_import() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let ta = a.get_text("t");
    ta.insert(0, "hello world").unwrap();
    a.commit();
    let f0 = a.state_frontiers();
    ta.insert(0, "XYZ").unwrap();
    a.commit();
    // checkout on the persistent calculator, before any import
    a.checkout(&f0).unwrap();
    a.checkout_to_latest();

    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let tb = b.get_text("t");

    let mut seed = 5u64;
    let mut cps = vec![];
    for round in 0..25 {
        for _ in 0..3 {
            let pa = below(&mut seed, ta.len_unicode() + 1);
            ta.insert(pa, "a").unwrap();
            let pb = below(&mut seed, tb.len_unicode() + 1);
            tb.insert(pb, "b").unwrap();
        }
        a.commit();
        b.commit();
        let ua = a.export(ExportMode::updates(&b.oplog_vv())).unwrap();
        let ub = b.export(ExportMode::updates(&a.oplog_vv())).unwrap();
        b.import(&ua).unwrap();
        a.import(&ub).unwrap();
        assert_eq!(ta.to_string(), tb.to_string(), "diverged round {round}");
        if round % 6 == 0 {
            cps.push(a.state_frontiers());
        }
    }

    let fresh = LoroDoc::new();
    fresh
        .import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let ft = fresh.get_text("t");
    for f in cps.iter().rev() {
        a.checkout(f).unwrap();
        fresh.checkout(f).unwrap();
        assert_eq!(ta.to_string(), ft.to_string());
    }
    a.checkout_to_latest();
    fresh.checkout_to_latest();
    assert_eq!(ta.to_string(), ft.to_string());
}

/// Three peers with asymmetric partial sync: plenty of merge points, none
/// of them single-head, and the meet is regularly non-critical — the
/// multi-head search and its fallbacks all get exercised.
#[test]
fn three_peer_crisscross() {
    let docs: Vec<LoroDoc> = (1..=3)
        .map(|i| {
            let d = LoroDoc::new();
            d.set_peer_id(i).unwrap();
            d
        })
        .collect();
    docs[0].get_text("t").insert(0, "criss cross base").unwrap();
    docs[0].commit();
    let all = docs[0].export(ExportMode::all_updates()).unwrap();
    docs[1].import(&all).unwrap();
    docs[2].import(&all).unwrap();

    let mut seed = 17u64;
    for round in 0..40 {
        for (i, d) in docs.iter().enumerate() {
            let t = d.get_text("t");
            let p = below(&mut seed, t.len_unicode() + 1);
            t.insert(p, &format!("{i}")).unwrap();
            if t.len_unicode() > 20 {
                let p = below(&mut seed, t.len_unicode() - 2);
                t.delete(p, 1).unwrap();
            }
            d.commit();
        }
        // partial, asymmetric sync: 0<->1 this round, 1<->2 next round
        let (x, y) = if round % 2 == 0 { (0, 1) } else { (1, 2) };
        let ux = docs[x]
            .export(ExportMode::updates(&docs[y].oplog_vv()))
            .unwrap();
        let uy = docs[y]
            .export(ExportMode::updates(&docs[x].oplog_vv()))
            .unwrap();
        docs[y].import(&ux).unwrap();
        docs[x].import(&uy).unwrap();
    }
    // full sync
    for _ in 0..3 {
        for i in 0..3 {
            for j in 0..3 {
                if i == j {
                    continue;
                }
                let u = docs[i]
                    .export(ExportMode::updates(&docs[j].oplog_vv()))
                    .unwrap();
                docs[j].import(&u).unwrap();
            }
        }
    }
    let s0 = docs[0].get_text("t").to_string();
    assert_eq!(s0, docs[1].get_text("t").to_string());
    assert_eq!(s0, docs[2].get_text("t").to_string());
    let fresh = LoroDoc::new();
    fresh
        .import(&docs[0].export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(fresh.get_text("t").to_string(), s0);
}

/// Alternating checkouts between two OLD versions after a big history.
/// merged = old_i ∪ old_j never includes the previously cached (latest) vv,
/// so every checkout should fall into the rebuild arm.
#[test]
fn alternating_old_checkouts() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    let ta = a.get_text("t");
    ta.insert(0, "alternating checkout base").unwrap();
    a.commit();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let tb = b.get_text("t");
    let mut seed = 31u64;
    let mut cps = vec![];
    for round in 0..30 {
        for _ in 0..3 {
            let pa = below(&mut seed, ta.len_unicode() + 1);
            ta.insert(pa, "a").unwrap();
            let pb = below(&mut seed, tb.len_unicode() + 1);
            tb.insert(pb, "b").unwrap();
        }
        a.commit();
        b.commit();
        let ua = a.export(ExportMode::updates(&b.oplog_vv())).unwrap();
        let ub = b.export(ExportMode::updates(&a.oplog_vv())).unwrap();
        b.import(&ua).unwrap();
        a.import(&ub).unwrap();
        if round % 5 == 0 {
            cps.push(a.state_frontiers());
        }
    }
    let fresh = LoroDoc::new();
    fresh
        .import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let ft = fresh.get_text("t");
    for _ in 0..3 {
        for f in cps.iter() {
            a.checkout(f).unwrap();
            fresh.checkout(f).unwrap();
            assert_eq!(ta.to_string(), ft.to_string(), "checkout mismatch");
        }
        for f in cps.iter().rev() {
            a.checkout(f).unwrap();
            fresh.checkout(f).unwrap();
            assert_eq!(ta.to_string(), ft.to_string(), "rev checkout mismatch");
        }
    }
}

/// Concurrently created text containers under the same map key: exercises
/// bring_back / new_containers together with the incremental path.
#[test]
fn concurrent_container_creation() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    a.get_map("m").insert("x", 1).unwrap();
    a.commit();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let ca = a
        .get_map("m")
        .insert_container("t", LoroText::new())
        .unwrap();
    ca.insert(0, "from a").unwrap();
    let cb = b
        .get_map("m")
        .insert_container("t", LoroText::new())
        .unwrap();
    cb.insert(0, "from b").unwrap();
    a.commit();
    b.commit();
    let ua = a.export(ExportMode::updates(&b.oplog_vv())).unwrap();
    let ub = b.export(ExportMode::updates(&a.oplog_vv())).unwrap();
    b.import(&ua).unwrap();
    a.import(&ub).unwrap();
    assert_eq!(
        a.get_deep_value().to_json(),
        b.get_deep_value().to_json(),
        "deep value diverged"
    );
    let fresh = LoroDoc::new();
    fresh
        .import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(
        fresh.get_deep_value().to_json(),
        a.get_deep_value().to_json()
    );

    // now keep editing the winner concurrently for a while
    let mut seed = 41u64;
    for round in 0..20 {
        let ta = a
            .get_map("m")
            .get("t")
            .unwrap()
            .into_container()
            .unwrap()
            .into_text()
            .unwrap();
        let tb = b
            .get_map("m")
            .get("t")
            .unwrap()
            .into_container()
            .unwrap()
            .into_text()
            .unwrap();
        let pa = below(&mut seed, ta.len_unicode() + 1);
        ta.insert(pa, "a").unwrap();
        let pb = below(&mut seed, tb.len_unicode() + 1);
        tb.insert(pb, "b").unwrap();
        a.commit();
        b.commit();
        let ua = a.export(ExportMode::updates(&b.oplog_vv())).unwrap();
        let ub = b.export(ExportMode::updates(&a.oplog_vv())).unwrap();
        b.import(&ua).unwrap();
        a.import(&ub).unwrap();
        assert_eq!(
            a.get_deep_value().to_json(),
            b.get_deep_value().to_json(),
            "diverged round {round}"
        );
    }
}

/// Two separate text containers, only one edited per round: makes sure the
/// affected set from `changed_containers_between` doesn't drop anything.
#[test]
fn two_texts_partial_rounds() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    for k in ["t1", "t2"] {
        a.get_text(k).insert(0, "base for ").unwrap();
        a.get_text(k).insert(9, k).unwrap();
    }
    a.commit();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let mut seed = 53u64;
    for round in 0..40 {
        let k = if round % 2 == 0 { "t1" } else { "t2" };
        let ta = a.get_text(k);
        let tb = b.get_text(k);
        let pa = below(&mut seed, ta.len_unicode() + 1);
        ta.insert(pa, "a").unwrap();
        let pb = below(&mut seed, tb.len_unicode() + 1);
        tb.insert(pb, "b").unwrap();
        a.commit();
        b.commit();
        let ua = a.export(ExportMode::updates(&b.oplog_vv())).unwrap();
        let ub = b.export(ExportMode::updates(&a.oplog_vv())).unwrap();
        b.import(&ua).unwrap();
        a.import(&ub).unwrap();
        for k in ["t1", "t2"] {
            assert_eq!(
                a.get_text(k).to_string(),
                b.get_text(k).to_string(),
                "{k} diverged round {round}"
            );
        }
    }
    let fresh = LoroDoc::new();
    fresh
        .import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    for k in ["t1", "t2"] {
        assert_eq!(fresh.get_text(k).to_string(), a.get_text(k).to_string());
    }
}

/// Marks + deletes + concurrent imports, then checkout back through history.
/// The styles vec is only extended on the incremental path.
#[test]
fn styles_with_checkout_history() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    let ta = a.get_text("t");
    ta.insert(0, "the quick brown fox jumps over the lazy dog")
        .unwrap();
    a.commit();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let tb = b.get_text("t");
    let mut seed = 67u64;
    let mut cps = vec![a.state_frontiers()];
    for round in 0..25 {
        let la = ta.len_unicode();
        let s = below(&mut seed, la - 3);
        let e = s + 1 + below(&mut seed, la - s - 1);
        ta.mark(s..e, "bold", true).unwrap();
        let lb = tb.len_unicode();
        let s2 = below(&mut seed, lb - 3);
        let e2 = s2 + 1 + below(&mut seed, lb - s2 - 1);
        tb.mark(s2..e2, "italic", true).unwrap();
        ta.insert(below(&mut seed, ta.len_unicode() + 1), "A")
            .unwrap();
        tb.insert(below(&mut seed, tb.len_unicode() + 1), "B")
            .unwrap();
        if round % 4 == 0 && ta.len_unicode() > 12 {
            ta.delete(below(&mut seed, ta.len_unicode() - 3), 2)
                .unwrap();
        }
        a.commit();
        b.commit();
        let ua = a.export(ExportMode::updates(&b.oplog_vv())).unwrap();
        let ub = b.export(ExportMode::updates(&a.oplog_vv())).unwrap();
        b.import(&ua).unwrap();
        a.import(&ub).unwrap();
        assert_eq!(
            ta.get_richtext_value().to_json(),
            tb.get_richtext_value().to_json(),
            "round {round}"
        );
        if round % 5 == 0 {
            cps.push(a.state_frontiers());
        }
    }
    let fresh = LoroDoc::new();
    fresh
        .import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let ft = fresh.get_text("t");
    for f in cps.iter().rev() {
        a.checkout(f).unwrap();
        fresh.checkout(f).unwrap();
        assert_eq!(
            ta.get_richtext_value().to_json(),
            ft.get_richtext_value().to_json(),
            "checkout richtext mismatch"
        );
    }
    a.checkout_to_latest();
    fresh.checkout_to_latest();
    assert_eq!(
        ta.get_richtext_value().to_json(),
        ft.get_richtext_value().to_json()
    );
}

/// Detached-mode imports interleaved with checkouts.
#[test]
fn detached_import_and_checkout() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    let ta = a.get_text("t");
    ta.insert(0, "detached base text").unwrap();
    a.commit();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let tb = b.get_text("t");
    let mut seed = 71u64;
    let mut cps = vec![];
    for round in 0..20 {
        for _ in 0..3 {
            let pa = below(&mut seed, ta.len_unicode() + 1);
            ta.insert(pa, "a").unwrap();
            let pb = below(&mut seed, tb.len_unicode() + 1);
            tb.insert(pb, "b").unwrap();
        }
        a.commit();
        b.commit();
        let ua = a.export(ExportMode::updates(&b.oplog_vv())).unwrap();
        let ub = b.export(ExportMode::updates(&a.oplog_vv())).unwrap();
        b.import(&ua).unwrap();
        a.import(&ub).unwrap();
        if round % 4 == 0 {
            cps.push(a.state_frontiers());
        }
        if round % 4 == 2 && !cps.is_empty() {
            // go detached, import while detached, then reattach
            a.checkout(cps.last().unwrap()).unwrap();
            let more = b.export(ExportMode::all_updates()).unwrap();
            a.import(&more).unwrap();
            a.checkout_to_latest();
        }
    }
    let fresh = LoroDoc::new();
    fresh
        .import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(fresh.get_text("t").to_string(), ta.to_string());
}

/// `get_cursor_pos` on a deleted target diffs from the delete op's deps to
/// the current version on a fresh persist-mode calculator. With a
/// conservative critical base that tracker keeps pre-base content as one
/// opaque span; the lookup still resolves because the replayed delete stamps
/// the target's real id onto that span. Pin the resolved position against a
/// fresh replica.
#[test]
fn cursor_on_deleted_target_after_concurrent_imports() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    let ta = a.get_text("t");
    ta.insert(0, "0123456789abcdefghij").unwrap();
    a.commit();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let tb = b.get_text("t");

    let cursor = ta.get_cursor(10, loro::cursor::Side::Left).unwrap();

    let mut seed = 23u64;
    for _ in 0..25 {
        for _ in 0..3 {
            let pa = below(&mut seed, ta.len_unicode() + 1);
            ta.insert(pa, "a").unwrap();
            let pb = below(&mut seed, tb.len_unicode() + 1);
            tb.insert(pb, "b").unwrap();
        }
        a.commit();
        b.commit();
        let ua = a.export(ExportMode::updates(&b.oplog_vv())).unwrap();
        let ub = b.export(ExportMode::updates(&a.oplog_vv())).unwrap();
        b.import(&ua).unwrap();
        a.import(&ub).unwrap();
    }
    // Delete the char the cursor is anchored to, forcing the
    // find-last-delete-op path through the diff calculator.
    ta.delete(0, ta.len_unicode()).unwrap();
    a.commit();

    let resolved = a.get_cursor_pos(&cursor).unwrap();
    let fresh = LoroDoc::new();
    fresh
        .import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let expected = fresh.get_cursor_pos(&cursor).unwrap();
    assert_eq!(resolved.current.pos, expected.current.pos);
}
