//! Replay-base selection: the bounded multi-head critical version search
//! (`OpLog::latest_critical_version_below_meet`) and its wiring in
//! `iter_from_replay_base_causally`.
//!
//! The DAG shape under test is the criss-cross that continuous two-peer sync
//! produces: every merge point has two heads, so the single-head descent
//! (`latest_single_head_critical_version`) stays stuck at the initial fork
//! point while the multi-head fixpoint finds the last fully synced version.

use crate::dag::DagUtils;
use crate::diff_calc::DiffMode;
use crate::loro::ExportMode;
use crate::oplog::CriticalVersionSearch;
use crate::{LoroDoc, VersionVector};

fn exchange(a: &LoroDoc, b: &LoroDoc) {
    let ua = a.export(ExportMode::updates(&b.oplog_vv())).unwrap();
    let ub = b.export(ExportMode::updates(&a.oplog_vv())).unwrap();
    a.import(&ub).unwrap();
    b.import(&ua).unwrap();
}

/// Two peers that edit concurrently and fully sync after every round.
fn criss_cross(rounds: usize) -> (LoroDoc, LoroDoc) {
    let a = LoroDoc::new_auto_commit();
    a.set_peer_id(1).unwrap();
    a.get_text("t").insert_unicode(0, "base").unwrap();
    a.commit_then_renew();
    let b = a.fork();
    b.set_peer_id(2).unwrap();
    for _ in 0..rounds {
        a.get_text("t").insert_unicode(0, "a").unwrap();
        a.commit_then_renew();
        b.get_text("t").insert_unicode(0, "b").unwrap();
        b.commit_then_renew();
        exchange(&a, &b);
    }
    (a, b)
}

/// One more unsynced concurrent pair on top of a synced criss-cross, with
/// `b`'s side imported into `a`. Returns `(a, sync_vv, before, after)`.
fn one_concurrent_import(rounds: usize) -> (LoroDoc, VersionVector, VersionVector, VersionVector) {
    let (a, b) = criss_cross(rounds);
    let sync_vv = a.oplog_vv();
    assert_eq!(sync_vv, b.oplog_vv());

    a.get_text("t").insert_unicode(0, "a").unwrap();
    a.commit_then_renew();
    b.get_text("t").insert_unicode(0, "b").unwrap();
    b.commit_then_renew();

    let before = a.oplog_vv();
    let update = b.export(ExportMode::updates(&before)).unwrap();
    a.import(&update).unwrap();
    let after = a.oplog_vv();
    (a, sync_vv, before, after)
}

/// Randomized multi-peer edit/sync workload: on ordinary concurrent
/// histories the search must always find a critical base — never fall back
/// to the single-head descent. (An earlier rounds-style cap on the fixpoint
/// caused a silent fallback on roughly a third of such imports.)
#[test]
fn randomized_sync_never_exhausts_the_search_budget() {
    use crate::oplog::CRITICAL_BASE_FALLBACK_COUNT;

    struct Lcg(u64);
    impl Lcg {
        fn below(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) as usize) % n
        }
    }
    let mut rng = Lcg(0xC0FFEE);

    let docs: Vec<LoroDoc> = (1..=3)
        .map(|p| {
            let d = LoroDoc::new_auto_commit();
            d.set_peer_id(p).unwrap();
            d
        })
        .collect();
    docs[0].get_text("t").insert_unicode(0, "seed").unwrap();
    docs[0].commit_then_renew();
    let all = docs[0].export(ExportMode::all_updates()).unwrap();
    docs[1].import(&all).unwrap();
    docs[2].import(&all).unwrap();

    let fallbacks_before = CRITICAL_BASE_FALLBACK_COUNT.with(|c| c.get());
    let rebuilds_before = crate::diff_calc::FULL_TRACKER_REBUILD_COUNT.with(|c| c.get());
    for _ in 0..600 {
        let i = rng.below(3);
        if rng.below(3) == 0 {
            let j = (i + 1 + rng.below(2)) % 3;
            let u = docs[i]
                .export(ExportMode::updates(&docs[j].oplog_vv()))
                .unwrap();
            docs[j].import(&u).unwrap();
        } else {
            let t = docs[i].get_text("t");
            let pos = rng.below(t.len_unicode() + 1);
            t.insert_unicode(pos, "x").unwrap();
            docs[i].commit_then_renew();
        }
    }
    for _ in 0..2 {
        for i in 0..3 {
            for j in 0..3 {
                if i != j {
                    let u = docs[i]
                        .export(ExportMode::updates(&docs[j].oplog_vv()))
                        .unwrap();
                    docs[j].import(&u).unwrap();
                }
            }
        }
    }
    assert_eq!(
        docs[0].get_text("t").to_string(),
        docs[1].get_text("t").to_string()
    );
    assert_eq!(
        docs[0].get_text("t").to_string(),
        docs[2].get_text("t").to_string()
    );
    let fallbacks = CRITICAL_BASE_FALLBACK_COUNT.with(|c| c.get()) - fallbacks_before;
    assert_eq!(
        fallbacks, 0,
        "the bounded search fell back to the single-head descent"
    );
    let rebuilds = crate::diff_calc::FULL_TRACKER_REBUILD_COUNT.with(|c| c.get()) - rebuilds_before;
    assert_eq!(
        rebuilds, 0,
        "randomized concurrent imports must not rebuild the tracker"
    );
}

#[test]
fn multi_head_critical_base_on_criss_cross_sync() {
    let (a, sync_vv, before, after) = one_concurrent_import(4);
    let before_frontiers = a.oplog().lock().dag.vv_to_frontiers(&before);
    let after_frontiers = a.oplog_frontiers();

    let oplog = a.oplog().lock();
    // On this DAG shape every synced version has two heads, so the single-head
    // descent stays stuck at the initial fork point (before peer 2's first op)
    // no matter how many rounds have passed…
    let single_head = oplog
        .dag
        .latest_single_head_critical_version(&before_frontiers, &after_frontiers);
    let single_head_vv = oplog.dag.frontiers_to_vv(&single_head).unwrap();
    assert!(!single_head_vv.includes_id(loro_common::ID::new(2, 0)));
    // …while the multi-head search retreats only to the last synced version.
    let (base, _) =
        oplog.iter_from_replay_base_causally(&before, &before_frontiers, &after, &after_frontiers);
    assert_eq!(base.vv, sync_vv);
    assert!(base.is_critical);
    assert_eq!(base.diff_mode, DiffMode::Import);
}

#[test]
fn fixpoint_returns_the_greatest_cut() {
    let (a, sync_vv, before, after) = one_concurrent_import(3);
    let mut merged = before.clone();
    merged.merge(&after);

    let oplog = a.oplog().lock();
    let res = oplog.latest_critical_version_below_meet(&before, &after, &merged);
    assert_eq!(res, CriticalVersionSearch::Found(sync_vv));
}

#[test]
fn disjoint_histories_have_no_cut_below_the_meet() {
    let a = LoroDoc::new_auto_commit();
    a.set_peer_id(1).unwrap();
    a.get_text("t").insert_unicode(0, "aaa").unwrap();
    a.commit_then_renew();
    let b = LoroDoc::new_auto_commit();
    b.set_peer_id(2).unwrap();
    b.get_text("t").insert_unicode(0, "bbb").unwrap();
    b.commit_then_renew();

    let before = a.oplog_vv();
    let before_frontiers = a.oplog_frontiers();
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let after = a.oplog_vv();
    let after_frontiers = a.oplog_frontiers();

    let oplog = a.oplog().lock();
    let mut merged = before.clone();
    merged.merge(&after);
    assert_eq!(
        oplog.latest_critical_version_below_meet(&before, &after, &merged),
        CriticalVersionSearch::NoneBelowMeet
    );
    // ∅ is trivially critical; the wiring reports it as such.
    let (base, _) =
        oplog.iter_from_replay_base_causally(&before, &before_frontiers, &after, &after_frontiers);
    assert!(base.vv.is_empty());
    assert!(base.is_critical);
}

// ---- Trusting a critical replay base (richtext) ----

/// peer 1 commits `insert + mark` in ONE change, so `StyleStart` and
/// `StyleEnd` are adjacent counters inside a single change. Peers 2 and 3
/// then fork *between* those two counters (detached editing on a mid-change
/// frontier), which is the only way a doc version can sit inside a style
/// pair. The greatest critical cut then lands between the two ops of one
/// mark, exercising the unpaired-end recovery path
/// (`style_for_end_anchor`) of the non-rebuilt tracker.
fn style_straddle_forks() -> (LoroDoc, LoroDoc, LoroDoc) {
    use crate::cursor::PosType;
    use crate::version::Frontiers;
    use loro_common::ID;

    let doc1 = LoroDoc::new_auto_commit();
    doc1.set_peer_id(1).unwrap();
    doc1.get_text("r")
        .insert(0, "hello world", PosType::Unicode)
        .unwrap();
    doc1.get_text("r")
        .mark(0, 5, "bold", true.into(), PosType::Unicode)
        .unwrap();
    doc1.commit_then_renew();
    let snapshot = doc1.export(ExportMode::Snapshot).unwrap();

    let mut forks = Vec::new();
    for (peer, s) in [(2u64, "AAA"), (3u64, "BBB")] {
        let d = LoroDoc::new_auto_commit();
        d.set_detached_editing(true);
        d.import(&snapshot).unwrap();
        d.set_peer_id(peer).unwrap();
        // Mid-change frontier: after StyleStart (counter 11), before
        // StyleEnd (counter 12).
        d.checkout(&Frontiers::from(ID::new(1, 11))).unwrap();
        d.set_peer_id(peer).unwrap();
        d.get_text("r").insert(2, s, PosType::Unicode).unwrap();
        d.commit_then_renew();
        forks.push(d);
    }

    let doc3 = forks.pop().unwrap();
    let doc2 = forks.pop().unwrap();
    (doc1, doc2, doc3)
}

#[test]
fn critical_base_may_cut_between_style_start_and_end() {
    let (doc1, doc2, doc3) = style_straddle_forks();
    let target = LoroDoc::new_auto_commit();
    target
        .import(&doc1.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    target
        .import(
            &doc2
                .export(ExportMode::updates(&target.oplog_vv()))
                .unwrap(),
        )
        .unwrap();

    let before = target.oplog_vv();
    let before_frontiers = target.oplog_frontiers();
    target
        .import(
            &doc3
                .export(ExportMode::updates(&target.oplog_vv()))
                .unwrap(),
        )
        .unwrap();
    let after = target.oplog_vv();
    let after_frontiers = target.oplog_frontiers();

    let oplog = target.oplog().lock();
    let (base, _) =
        oplog.iter_from_replay_base_causally(&before, &before_frontiers, &after, &after_frontiers);
    // A later change to the base search that rounds cuts to change (or style
    // pair) boundaries would silently drop this coverage — keep it pinned.
    assert_eq!(
        base.vv.get(&1).copied(),
        Some(12),
        "base must cut between StyleStart(11) and StyleEnd(12); got {:?}",
        base.vv
    );
    assert!(
        base.is_critical,
        "the mid-mark cut must be reported critical"
    );
    assert_ne!(base.vv, before, "must be a conservative base");
}

#[test]
fn style_straddling_base_converges_in_every_import_order() {
    use crate::diff_calc::FULL_TRACKER_REBUILD_COUNT;
    use crate::version::Frontiers;

    let (doc1, doc2, doc3) = style_straddle_forks();
    let rebuilds_before = FULL_TRACKER_REBUILD_COUNT.with(|c| c.get());
    // a: import 2 then 3. b: import 3 then 2. c: everything in one batch.
    let a = LoroDoc::new_auto_commit();
    a.import(&doc1.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    a.import(&doc2.export(ExportMode::updates(&a.oplog_vv())).unwrap())
        .unwrap();
    a.import(&doc3.export(ExportMode::updates(&a.oplog_vv())).unwrap())
        .unwrap();

    let b = LoroDoc::new_auto_commit();
    b.import(&doc1.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    b.import(&doc3.export(ExportMode::updates(&b.oplog_vv())).unwrap())
        .unwrap();
    b.import(&doc2.export(ExportMode::updates(&b.oplog_vv())).unwrap())
        .unwrap();

    let rebuilds = FULL_TRACKER_REBUILD_COUNT.with(|c| c.get()) - rebuilds_before;
    assert_eq!(
        rebuilds, 0,
        "the straddle imports must take the trusted path"
    );

    let c = LoroDoc::new_auto_commit();
    c.import(&doc1.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    let all = vec![
        doc2.export(ExportMode::updates(&c.oplog_vv())).unwrap(),
        doc3.export(ExportMode::updates(&c.oplog_vv())).unwrap(),
    ];
    c.import_batch(&all).unwrap();

    assert_eq!(a.get_text("r").to_string(), b.get_text("r").to_string());
    assert_eq!(a.get_text("r").to_string(), c.get_text("r").to_string());
    assert_eq!(
        a.get_text("r").get_richtext_value(),
        b.get_text("r").get_richtext_value()
    );
    assert_eq!(
        a.get_text("r").get_richtext_value(),
        c.get_text("r").get_richtext_value()
    );

    // Independent oracle: a checkout round-trip through the empty frontiers
    // forces a full-history rebuild, so the trusted incremental result is
    // compared against the rebuild path — not just against itself.
    let incremental = a.get_text("r").get_richtext_value();
    let head = a.oplog_frontiers();
    let oracle_before = FULL_TRACKER_REBUILD_COUNT.with(|c| c.get());
    a.checkout(&Frontiers::default()).unwrap();
    a.checkout(&head).unwrap();
    assert!(
        FULL_TRACKER_REBUILD_COUNT.with(|c| c.get()) > oracle_before,
        "the oracle round-trip must take the rebuild path"
    );
    assert_eq!(a.get_text("r").get_richtext_value(), incremental);
}

/// Continuous two-peer sync with marks: text and styles converge, and the
/// full-history tracker rebuild never fires — every concurrent import is
/// served from a tracker seeded at the (multi-head critical) replay base.
#[test]
fn criss_cross_sync_never_rebuilds_the_text_tracker() {
    use crate::cursor::PosType;
    use crate::diff_calc::FULL_TRACKER_REBUILD_COUNT;

    let a = LoroDoc::new_auto_commit();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new_auto_commit();
    b.set_peer_id(2).unwrap();
    a.get_text("r")
        .insert(0, "seed text here", PosType::Unicode)
        .unwrap();
    a.commit_then_renew();
    b.import(&a.export(ExportMode::updates(&b.oplog_vv())).unwrap())
        .unwrap();

    let rebuilds_before = FULL_TRACKER_REBUILD_COUNT.with(|c| c.get());
    for round in 0..40 {
        for (d, s) in [(&a, "x"), (&b, "y")] {
            let t = d.get_text("r");
            t.insert(1, s, PosType::Unicode).unwrap();
            let len = t.len_unicode();
            t.mark(
                0,
                (len / 2).max(1),
                "bold",
                (round % 2 == 0).into(),
                PosType::Unicode,
            )
            .unwrap();
            d.commit_then_renew();
        }
        exchange(&a, &b);
    }
    assert_eq!(a.get_text("r").to_string(), b.get_text("r").to_string());
    assert_eq!(
        a.get_text("r").get_richtext_value(),
        b.get_text("r").get_richtext_value()
    );
    let rebuilds = FULL_TRACKER_REBUILD_COUNT.with(|c| c.get()) - rebuilds_before;
    assert_eq!(
        rebuilds, 0,
        "concurrent imports must not rebuild the tracker"
    );

    // Independent oracle: force one rebuild via a checkout round-trip and
    // compare it against the incrementally maintained state.
    let incremental = a.get_text("r").get_richtext_value();
    let head = a.oplog_frontiers();
    a.checkout(&crate::version::Frontiers::default()).unwrap();
    a.checkout(&head).unwrap();
    assert!(
        FULL_TRACKER_REBUILD_COUNT.with(|c| c.get()) > rebuilds_before,
        "the oracle round-trip must take the rebuild path"
    );
    assert_eq!(a.get_text("r").get_richtext_value(), incremental);
}

/// A persisted calculator replays ops the tracker has already applied
/// whenever the replay base sits below the version the tracker reached on
/// an earlier round (the shape `LoroDoc::checkout` produces — driven here
/// directly through one persisted `DiffCalculator`). The tracker skips
/// them (`skip_applied`), and the style table must not grow a duplicate
/// entry per re-replayed StyleStart: trusting the base turns what was a
/// once-per-retreat re-replay into one on every step.
#[test]
fn persisted_walks_do_not_duplicate_style_entries() {
    use crate::cursor::PosType;
    use crate::diff_calc::{ContainerDiffCalculator, DiffCalculator, FULL_TRACKER_REBUILD_COUNT};
    use crate::handler::HandlerTrait;

    let a = LoroDoc::new_auto_commit();
    a.set_peer_id(1).unwrap();
    a.get_text("t")
        .insert(0, "seed text", PosType::Unicode)
        .unwrap();
    a.commit_then_renew();
    let b = a.fork();
    b.set_peer_id(2).unwrap();

    // A forward checkout path through half-synced frontiers: `{A_i}` is not
    // critical (B_i is concurrent), so the step to `{A_i, B_i}` replays from
    // the previous sync point and re-applies A_i's ops.
    let rounds = 6;
    let mut stops = vec![a.oplog_frontiers()];
    for round in 0..rounds {
        for (d, s) in [(&a, "x"), (&b, "y")] {
            let t = d.get_text("t");
            t.insert(0, s, PosType::Unicode).unwrap();
            t.mark(0, 4, "bold", (round % 2 == 0).into(), PosType::Unicode)
                .unwrap();
            d.commit_then_renew();
        }
        stops.push(a.oplog_frontiers());
        exchange(&a, &b);
        stops.push(a.oplog_frontiers());
    }

    let oplog = a.oplog().lock();
    let idx = a.get_text("t").idx();
    let mut calc = DiffCalculator::new(true);
    let rebuilds_before = FULL_TRACKER_REBUILD_COUNT.with(|c| c.get());
    for w in stops.windows(2) {
        let before = oplog.dag.frontiers_to_vv(&w[0]).unwrap();
        let after = oplog.dag.frontiers_to_vv(&w[1]).unwrap();
        calc.calc_diff_internal(&oplog, &before, &w[0], &after, &w[1], None);
    }
    assert_eq!(
        FULL_TRACKER_REBUILD_COUNT.with(|c| c.get()) - rebuilds_before,
        0,
        "a forward walk over critical bases must not rebuild (a rebuild would reset the style table and mask the leak)"
    );

    let depth = oplog.arena.get_depth(idx);
    let (_, c) = calc.get_or_create_calc(idx, depth);
    let ContainerDiffCalculator::Richtext(text) = c else {
        panic!("expected a richtext calculator");
    };
    assert_eq!(
        text.tracked_style_ids().len(),
        2 * rounds,
        "each StyleStart must appear exactly once in the style table"
    );
}

/// The StyleEnd fallback (`style_for_end_anchor`) pushes a style entry when
/// its StyleStart lies below the tracker's seed. Re-replaying that StyleEnd
/// must reuse the entry, not push another; likewise a rebuild-created entry
/// must be reused by a later re-replay.
#[test]
fn straddling_walks_never_duplicate_style_entries() {
    use crate::diff_calc::{ContainerDiffCalculator, DiffCalculator, FULL_TRACKER_REBUILD_COUNT};
    use crate::handler::HandlerTrait;

    let (doc1, doc2, doc3) = style_straddle_forks();
    // A second mark on one fork puts a StyleStart into the region the later
    // steps re-replay (the straddling mark's own StyleStart sits below every
    // base and only its StyleEnd is replayed, via the fallback).
    doc2.get_text("r")
        .mark(0, 3, "italic", true.into(), crate::cursor::PosType::Unicode)
        .unwrap();
    doc2.commit_then_renew();
    let target = LoroDoc::new_auto_commit();
    for d in [&doc1, &doc2, &doc3] {
        target
            .import(&d.export(ExportMode::updates(&target.oplog_vv())).unwrap())
            .unwrap();
    }
    let f0 = doc1.oplog_frontiers();
    let f1 = {
        let t = doc1.fork();
        t.import(&doc2.export(ExportMode::updates(&t.oplog_vv())).unwrap())
            .unwrap();
        t.oplog_frontiers()
    };
    let f2 = target.oplog_frontiers();

    let oplog = target.oplog().lock();
    let idx = target.get_text("r").idx();
    let mut calc = DiffCalculator::new(true);
    let rebuilds_before = FULL_TRACKER_REBUILD_COUNT.with(|c| c.get());
    // Forward over a base that cuts between StyleStart and StyleEnd (the
    // fallback pushes), forward again (the fallback must reuse), retreat
    // (rebuild replaces the table), forward again (reuse after a rebuild).
    for w in [[&f0, &f1], [&f1, &f2], [&f2, &f1], [&f1, &f2]] {
        let before = oplog.dag.frontiers_to_vv(w[0]).unwrap();
        let after = oplog.dag.frontiers_to_vv(w[1]).unwrap();
        calc.calc_diff_internal(&oplog, &before, w[0], &after, w[1], None);

        let depth = oplog.arena.get_depth(idx);
        let (_, c) = calc.get_or_create_calc(idx, depth);
        let ContainerDiffCalculator::Richtext(text) = c else {
            panic!("expected a richtext calculator");
        };
        let mut ids = text.tracked_style_ids();
        let len = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), len, "duplicate style entry for one op id");
        assert_eq!(len, 2, "one entry per mark: the fallback's and the fork's");
    }
    assert_eq!(
        FULL_TRACKER_REBUILD_COUNT.with(|c| c.get()) - rebuilds_before,
        1,
        "only the retreating step may rebuild"
    );
}

/// Same criss-cross shape on a plain list container: the list tracker also
/// stops rebuilding once the replay base is certified critical.
#[test]
fn criss_cross_sync_never_rebuilds_the_list_tracker() {
    use crate::diff_calc::FULL_TRACKER_REBUILD_COUNT;

    let a = LoroDoc::new_auto_commit();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new_auto_commit();
    b.set_peer_id(2).unwrap();
    a.get_list("l").insert(0, "seed").unwrap();
    a.commit_then_renew();
    b.import(&a.export(ExportMode::updates(&b.oplog_vv())).unwrap())
        .unwrap();

    let rebuilds_before = FULL_TRACKER_REBUILD_COUNT.with(|c| c.get());
    for round in 0..40 {
        for (d, v) in [(&a, "x"), (&b, "y")] {
            d.get_list("l").insert(0, format!("{v}{round}")).unwrap();
            d.commit_then_renew();
        }
        exchange(&a, &b);
    }
    assert_eq!(a.get_deep_value(), b.get_deep_value());
    let rebuilds = FULL_TRACKER_REBUILD_COUNT.with(|c| c.get()) - rebuilds_before;
    assert_eq!(
        rebuilds, 0,
        "concurrent list imports must not rebuild the tracker"
    );
}
