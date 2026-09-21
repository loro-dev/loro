//! The shallow root must be a *critical version* of the exported document:
//! every op is either causally before it or causally after it, never
//! concurrent with it.
//!
//! The shallow format stores one state snapshot at the root and every op after
//! it. An op concurrent with the root belongs to neither side, so a root that
//! is merely a common ancestor of the frontier heads is not enough — see
//! `crates/loro-internal/docs/critical-version-spec.md` and loro-dev/loro#1095,
//! where an unpaired frontier head was used as the root and the resulting
//! snapshot could not be imported.

use loro::{ExportMode, Frontiers, LoroDoc, ID};
use rand::{rngs::StdRng, Rng, SeedableRng};

/// Why a frontier fails to be a critical version of `doc`.
#[derive(Debug)]
struct NotCritical {
    op: ID,
    reason: &'static str,
}

/// Checks the defining property directly: for every op in the document, the op
/// is in the root's causal past, or the root is in the op's causal past.
fn check_critical(doc: &LoroDoc, root: &Frontiers) -> Result<(), NotCritical> {
    if root.is_empty() {
        // The empty version trims nothing, so nothing can be concurrent with it.
        return Ok(());
    }

    let vv_root = doc.frontiers_to_vv(root).ok_or(NotCritical {
        op: root.iter().next().unwrap(),
        reason: "root is not in the dag",
    })?;

    for (peer, end) in doc.oplog_vv().iter() {
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

/// Asserts the export contract for `doc` at `frontiers`: the chosen root is a
/// critical version, and the blob round-trips into a fresh document.
fn assert_export_contract(doc: &LoroDoc, label: &str) {
    let frontiers = doc.oplog_frontiers();
    if frontiers.is_empty() {
        return;
    }
    let expected = doc.get_text("t").to_string();

    for (mode_name, blob) in [
        (
            "shallow_snapshot",
            doc.export(ExportMode::shallow_snapshot(&frontiers)).unwrap(),
        ),
        (
            "state_only",
            doc.export(ExportMode::state_only(Some(&frontiers))).unwrap(),
        ),
    ] {
        let meta = LoroDoc::decode_import_blob_meta(&blob, false).unwrap();
        if let Err(bad) = check_critical(doc, &meta.start_frontiers) {
            panic!(
                "{label}: {mode_name} picked a non-critical root {:?} \
                 (frontiers {frontiers:?}): {} {:?}",
                meta.start_frontiers, bad.reason, bad.op
            );
        }

        let fresh = LoroDoc::new();
        fresh.import(&blob).unwrap_or_else(|e| {
            panic!(
                "{label}: {mode_name} blob with root {:?} failed to import: {e:?}",
                meta.start_frontiers
            )
        });
        assert_eq!(
            fresh.get_text("t").to_string(),
            expected,
            "{label}: {mode_name} blob restored the wrong content"
        );
    }
}

/// Builds a random causal graph by interleaving local commits with partial
/// syncs, which is what produces multi-head frontiers and criss-cross merges.
fn random_docs(rng: &mut StdRng, peers: u64, steps: usize) -> Vec<LoroDoc> {
    let docs: Vec<LoroDoc> = (0..peers)
        .map(|i| {
            let d = LoroDoc::new();
            d.set_peer_id(i + 1).unwrap();
            d
        })
        .collect();

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
    }

    docs
}

#[test]
fn shallow_root_is_critical_on_random_histories() {
    let cases: usize = std::env::var("LORO_SHALLOW_ROOT_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);

    for case in 0..cases {
        let mut rng = StdRng::seed_from_u64(0x10_95_00_00 + case as u64);
        let peers = rng.gen_range(2..=5);
        let steps = rng.gen_range(4..24);
        for (i, doc) in random_docs(&mut rng, peers, steps).into_iter().enumerate() {
            assert_export_contract(&doc, &format!("case {case} peer {i}"));
        }
    }
}

/// The shape from loro-dev/loro#1095: independent heads have no common
/// ancestor at all, so no critical version exists below them and the export
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
        assert_export_contract(&doc, &format!("{peers} independent heads"));

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
    assert_export_contract(&base, "shared root, 5 heads");
}
