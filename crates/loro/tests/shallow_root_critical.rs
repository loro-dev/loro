//! The shallow root must be a *critical version* of the exported document:
//! every op is either causally before it or causally after it, never
//! concurrent with it.
//!
//! The shallow format stores one state snapshot at the root and every op after
//! it. An op concurrent with the root belongs to neither side, so a root that
//! is merely the meet of the frontier heads is not enough — see
//! `crates/loro-internal/docs/critical-version-spec.md` and loro-dev/loro#1095,
//! where an unpaired frontier head was used as the root and the resulting
//! snapshot could not be imported.

use loro::{ExportMode, Frontiers, LoroDoc, VersionVector, ID};
use rand::{rngs::StdRng, Rng, SeedableRng};

/// Why a frontier fails to be a critical version of `doc`.
#[derive(Debug)]
struct NotCritical {
    op: ID,
    reason: &'static str,
}

/// Checks the defining property directly: for every op in `region`, the op is
/// in the root's causal past, or the root is in the op's causal past.
fn check_critical(
    doc: &LoroDoc,
    root: &Frontiers,
    region: &VersionVector,
) -> Result<(), NotCritical> {
    if root.is_empty() {
        // The empty version trims nothing, so nothing can be concurrent with it.
        return Ok(());
    }

    let vv_root = doc.frontiers_to_vv(root).ok_or(NotCritical {
        op: root.iter().next().unwrap(),
        reason: "root is not in the dag",
    })?;

    for (peer, end) in region.iter() {
        for counter in 0..*end {
            let op = ID::new(*peer, counter);
            if vv_root.get(peer).copied().unwrap_or(0) > counter {
                continue; // before the root
            }
            let Some(vv_op) = doc.frontiers_to_vv(&Frontiers::from(op)) else {
                return Err(NotCritical {
                    op,
                    reason: "op is not in the dag",
                });
            };
            if !vv_op.includes_vv(&vv_root) {
                return Err(NotCritical {
                    op,
                    reason: "op is concurrent with the root",
                });
            }
        }
    }

    Ok(())
}

/// Asserts the export contract at `target`, which may be any version of `doc`.
///
/// `shallow_snapshot(target)` keeps every op from the root up to the latest
/// version, so the root must be critical for the whole document.
/// `state_only(target)` keeps ops only up to `target`, so the root must be
/// critical for `target`'s history. Either way the blob must import into a
/// fresh document and restore the right content.
fn assert_export_contract(doc: &LoroDoc, target: &Frontiers, label: &str) {
    if target.is_empty() {
        return;
    }
    let whole = doc.oplog_vv();
    let at_target = doc.frontiers_to_vv(target).unwrap();
    let latest_text = doc.get_text("t").to_string();
    let target_text = doc.fork_at(target).unwrap().get_text("t").to_string();

    for (mode_name, blob, region, expected) in [
        (
            "shallow_snapshot",
            doc.export(ExportMode::shallow_snapshot(target)).unwrap(),
            &whole,
            &latest_text,
        ),
        (
            "state_only",
            doc.export(ExportMode::state_only(Some(target))).unwrap(),
            &at_target,
            &target_text,
        ),
    ] {
        let meta = LoroDoc::decode_import_blob_meta(&blob, false).unwrap();
        if let Err(bad) = check_critical(doc, &meta.start_frontiers, region) {
            panic!(
                "{label}: {mode_name}({target:?}) picked a non-critical root {:?} \
                 (latest {:?}): {} {:?}",
                meta.start_frontiers,
                doc.oplog_frontiers(),
                bad.reason,
                bad.op
            );
        }

        let fresh = LoroDoc::new();
        fresh.import(&blob).unwrap_or_else(|e| {
            panic!(
                "{label}: {mode_name}({target:?}) blob with root {:?} failed to import: {e:?}",
                meta.start_frontiers
            )
        });
        assert_eq!(
            &fresh.get_text("t").to_string(),
            expected,
            "{label}: {mode_name}({target:?}) restored the wrong content"
        );
    }
}

/// Builds a random causal graph by interleaving local commits with partial
/// syncs, which is what produces multi-head frontiers and criss-cross merges.
fn random_docs(rng: &mut StdRng, peers: u64, steps: usize) -> (Vec<LoroDoc>, Vec<Frontiers>) {
    let docs: Vec<LoroDoc> = (0..peers)
        .map(|i| {
            let d = LoroDoc::new();
            d.set_peer_id(i + 1).unwrap();
            d
        })
        .collect();

    let mut history = Vec::new();
    for step in 0..steps {
        let i = rng.gen_range(0..docs.len());
        if peers > 1 && rng.gen_ratio(1, 3) {
            let j = rng.gen_range(0..docs.len());
            if i != j {
                let updates = docs[j]
                    .export(ExportMode::updates(&docs[i].oplog_vv()))
                    .unwrap();
                docs[i].import(&updates).unwrap();
            }
        } else {
            docs[i]
                .get_text("t")
                .insert(0, &format!("{}", step % 10))
                .unwrap();
            docs[i].commit();
        }
        history.push(docs[i].oplog_frontiers());
    }

    (docs, history)
}

#[test]
fn shallow_root_is_critical_on_random_histories() {
    let cases: usize = std::env::var("LORO_SHALLOW_ROOT_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(150);

    for case in 0..cases {
        let mut rng = StdRng::seed_from_u64(0x10_95_00_00 + case as u64);
        let peers = rng.gen_range(2..=5);
        let steps = rng.gen_range(4..24);
        let (docs, history) = random_docs(&mut rng, peers, steps);
        for (i, doc) in docs.iter().enumerate() {
            let label = format!("case {case} peer {i}");
            assert_export_contract(doc, &doc.oplog_frontiers(), &label);
            // Past versions: later ops may branch off below the chosen root.
            let known: Vec<&Frontiers> = history
                .iter()
                .filter(|f| doc.frontiers_to_vv(f).is_some())
                .collect();
            for _ in 0..known.len().min(3) {
                let past = known[rng.gen_range(0..known.len())];
                assert_export_contract(doc, past, &label);
            }
        }
    }
}

/// The shape from loro-dev/loro#1095: independent heads share no history at
/// all, so no critical version exists below them and the export
/// must fall back to full history.
#[test]
fn independent_heads_have_no_critical_root() {
    for peers in 1..=9u64 {
        let doc = LoroDoc::new();
        doc.set_peer_id(1).unwrap();
        doc.get_text("t").insert(0, "a").unwrap();
        doc.commit();
        for p in 2..=peers {
            let other = LoroDoc::new();
            other.set_peer_id(p).unwrap();
            other.get_text("t").insert(0, "b").unwrap();
            other.commit();
            doc.import(&other.export(ExportMode::all_updates()).unwrap())
                .unwrap();
        }
        assert_eq!(doc.oplog_frontiers().len(), peers as usize);
        assert_export_contract(
            &doc,
            &doc.oplog_frontiers(),
            &format!("{peers} independent heads"),
        );

        let blob = doc
            .export(ExportMode::shallow_snapshot(&doc.oplog_frontiers()))
            .unwrap();
        let meta = LoroDoc::decode_import_blob_meta(&blob, false).unwrap();
        if peers > 1 {
            assert!(
                meta.start_frontiers.is_empty(),
                "{peers} independent heads admit no critical version, \
                 so the export must keep the full history, got {:?}",
                meta.start_frontiers
            );
        }
    }
}

/// A shared root makes a critical version exist again, so the export must
/// still trim. Guards against "fall back to full history" becoming the answer
/// for every multi-head document.
#[test]
fn shared_root_multi_head_still_trims() {
    let base = LoroDoc::new();
    base.set_peer_id(100).unwrap();
    base.get_text("t").insert(0, "root").unwrap();
    base.commit();
    let root = base.oplog_frontiers();
    let snapshot = base.export(ExportMode::Snapshot).unwrap();

    for p in 1..=5u64 {
        let fork = LoroDoc::new();
        fork.set_peer_id(p).unwrap();
        fork.import(&snapshot).unwrap();
        fork.get_text("t").insert(0, "x").unwrap();
        fork.commit();
        base.import(&fork.export(ExportMode::all_updates()).unwrap())
            .unwrap();
    }
    assert_eq!(base.oplog_frontiers().len(), 5);

    let blob = base
        .export(ExportMode::shallow_snapshot(&base.oplog_frontiers()))
        .unwrap();
    let meta = LoroDoc::decode_import_blob_meta(&blob, false).unwrap();
    assert_eq!(meta.start_frontiers, root, "must trim to the shared root");
    assert_export_contract(&base, &base.oplog_frontiers(), "shared root, 5 heads");
}

/// Exporting at a past version keeps every op up to the latest one. A branch
/// merged later that forked below the requested version is concurrent with
/// it, so the requested version itself is not a valid root even though it has
/// a single head.
#[test]
fn past_version_with_later_branch_below_it() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.get_text("t").insert(0, "0").unwrap();
    doc.commit();
    let at_a0 = doc.export(ExportMode::Snapshot).unwrap();
    doc.get_text("t").insert(0, "1").unwrap();
    doc.commit();
    doc.get_text("t").insert(0, "2").unwrap();
    doc.commit();
    let target = doc.oplog_frontiers();

    let fork = LoroDoc::new();
    fork.set_peer_id(2).unwrap();
    fork.import(&at_a0).unwrap();
    fork.get_text("t").insert(1, "B").unwrap();
    fork.commit();
    doc.import(&fork.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(doc.oplog_frontiers().len(), 2);

    assert_export_contract(&doc, &target, "past single head, later branch below it");
}
