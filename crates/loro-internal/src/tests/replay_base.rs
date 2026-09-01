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
