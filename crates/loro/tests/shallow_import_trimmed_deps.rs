// Regression cases adapted from rexikan's unmerged PR #1083.
use loro::{Counter, ExportMode, Frontiers, LoroDoc, LoroError, VersionVector, ID};

/// Counter of the shallow root in `import_fork_into_shallow_doc`.
const SHALLOW_ROOT: Counter = 2;

/// Peer 1 writes three single-char ops. A shallow snapshot at `2@1` keeps
/// `2@1` as the root and trims `0@1` and `1@1`. Peer 2 forks at `fork@1`
/// and writes one op, whose deps are therefore `[fork@1]`. Returns the
/// shallow doc's import result for that op together with the doc.
fn import_fork_into_shallow_doc(fork: Counter) -> (LoroDoc, loro::LoroResult<()>) {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    for i in 0..=SHALLOW_ROOT {
        doc.get_text("t").insert(i as usize, "a").unwrap();
        doc.commit();
    }

    let root = Frontiers::from(ID::new(1, SHALLOW_ROOT));
    let snap = doc.export(ExportMode::shallow_snapshot(&root)).unwrap();
    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&snap).unwrap();
    assert_eq!(shallow_doc.shallow_since_frontiers(), root);

    let forked = doc.fork_at(&Frontiers::from(ID::new(1, fork))).unwrap();
    forked.set_peer_id(2).unwrap();
    forked.get_text("t").insert(0, "b").unwrap();
    forked.commit();
    let update = forked.export(ExportMode::updates(&doc.oplog_vv())).unwrap();
    let result = shallow_doc.import(&update).map(|_| ());
    (shallow_doc, result)
}

/// Deps strictly below the shallow root's deps are rejected.
#[test]
fn shallow_doc_rejects_op_depending_below_root() {
    let (_, result) = import_fork_into_shallow_doc(SHALLOW_ROOT - 2);
    assert_eq!(
        result.unwrap_err(),
        LoroError::ImportUpdatesThatDependsOnOutdatedVersion
    );
}

/// Deps equal to the shallow root's own deps (`[1@1]`) make the op concurrent
/// with the root: the doc holds no state without the root op, and the dag has
/// no node, hence no lamport, for a trimmed id. This used to pass the
/// preflight, get parked as pending, and abort on
/// `calc_unknown_lamport_change(..).unwrap()` when the pending replay applied
/// it, poisoning the doc mutex.
#[test]
fn shallow_doc_rejects_op_depending_on_root_deps() {
    let (shallow_doc, result) = import_fork_into_shallow_doc(SHALLOW_ROOT - 1);
    assert_eq!(
        result.unwrap_err(),
        LoroError::ImportUpdatesThatDependsOnOutdatedVersion
    );
    // The rejected import left no trace and the doc stays usable.
    assert_eq!(
        shallow_doc.oplog_frontiers(),
        Frontiers::from(ID::new(1, SHALLOW_ROOT))
    );
    assert_eq!(shallow_doc.get_text("t").to_string(), "aaa");
    shallow_doc.get_text("t").insert(0, "c").unwrap();
    shallow_doc.commit();
    assert_eq!(shallow_doc.get_text("t").to_string(), "caaa");
}

/// Deps on the shallow root itself are importable.
#[test]
fn shallow_doc_accepts_op_depending_on_root() {
    let (shallow_doc, result) = import_fork_into_shallow_doc(SHALLOW_ROOT);
    result.unwrap();
    assert_eq!(shallow_doc.get_text("t").to_string(), "baaa");
}

/// Deps mixing the shallow root with a trimmed id of another peer cannot come
/// from loro itself (a frontier holds one id per peer and is minimal), but
/// they can come from hand-written JSON. They used to slip past the preflight
/// through its boundary special case and abort like the case above.
#[test]
fn shallow_doc_rejects_json_op_mixing_root_with_trimmed_dep() {
    // Trimmed history spanning two peers: on top of the shallow doc holding
    // `2@1` and peer 2's `0@2`, peer 3 writes one op and the doc is
    // re-exported shallow at that op.
    let (shallow_doc, result) = import_fork_into_shallow_doc(SHALLOW_ROOT);
    result.unwrap();
    shallow_doc.set_peer_id(3).unwrap();
    shallow_doc.get_text("t").insert(0, "c").unwrap();
    shallow_doc.commit();
    let root = Frontiers::from(ID::new(3, 0));
    let snap = shallow_doc
        .export(ExportMode::shallow_snapshot(&root))
        .unwrap();
    let nested = LoroDoc::new();
    nested.import(&snap).unwrap();
    let trimmed = nested.shallow_since_vv().to_vv();
    assert!(trimmed.includes_id(ID::new(1, SHALLOW_ROOT)));
    assert!(trimmed.includes_id(ID::new(2, 0)));

    let other = LoroDoc::new();
    other.set_peer_id(7).unwrap();
    other.get_text("t").insert(0, "x").unwrap();
    other.commit();
    let mut json = other
        .export_json_updates_without_peer_compression(&VersionVector::default(), &other.oplog_vv())
        .unwrap();
    json.changes[0].deps = vec![ID::new(3, 0), ID::new(1, SHALLOW_ROOT)];

    assert_eq!(
        nested.import_json_updates(json).unwrap_err(),
        LoroError::ImportUpdatesThatDependsOnOutdatedVersion
    );
    assert_eq!(nested.get_text("t").to_string(), "cbaaa");
}

// Issue #1068's per-peer map update, exercised through every import entry point.
#[test]
fn folded_peer_update_returns_error_through_all_import_apis() {
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    b.get_map("m").insert("k0", "v0").unwrap();
    b.commit();
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    a.get_map("m").insert("k1", "v1").unwrap();
    a.commit();
    let snapshot = a
        .export(ExportMode::shallow_snapshot(&a.oplog_frontiers()))
        .unwrap();
    let base = b.oplog_vv();
    b.get_map("m").insert("k2", "v2").unwrap();
    b.commit();
    let update = b.export(ExportMode::updates(&base)).unwrap();
    let json = b.export_json_updates(&base, &b.oplog_vv()).unwrap();
    let empty = LoroDoc::new().export(ExportMode::all_updates()).unwrap();
    for detached in [false, true] {
        for api in 0..3 {
            let doc = LoroDoc::new();
            doc.import(&snapshot).unwrap();
            let before = doc.get_deep_value();
            let vv = doc.oplog_vv();
            if detached {
                doc.detach();
            }
            let result = match api {
                0 => doc.import(&update),
                1 => doc.import_json_updates(json.clone()),
                _ => doc.import_batch(&[update.clone(), empty.clone()]),
            };
            assert_eq!(
                result.unwrap_err(),
                LoroError::ImportUpdatesThatDependsOnOutdatedVersion
            );
            assert_eq!(doc.is_detached(), detached);
            assert_eq!(doc.oplog_vv(), vv);
            assert_eq!(doc.get_deep_value(), before);
            assert!(doc.export_pending_updates().unwrap().is_none());
            doc.try_attach().unwrap();
            doc.get_map("m").insert("usable", true).unwrap();
            doc.commit();
            let roundtrip = LoroDoc::new();
            roundtrip
                .import(&doc.export(ExportMode::Snapshot).unwrap())
                .unwrap();
            assert_eq!(roundtrip.get_deep_value(), doc.get_deep_value());
        }
    }
}

#[test]
fn trimmed_dep_with_future_dep_is_rejected_instead_of_parked() {
    let source = LoroDoc::new();
    source.set_peer_id(1).unwrap();
    source.get_map("m").insert("base", 1).unwrap();
    source.commit();
    source.get_map("m").insert("root", 2).unwrap();
    source.commit();
    let snapshot = source
        .export(ExportMode::shallow_snapshot(&source.oplog_frontiers()))
        .unwrap();
    let other = LoroDoc::new();
    other.set_peer_id(7).unwrap();
    other.get_map("m").insert("other", 3).unwrap();
    other.commit();
    let mut json = other
        .export_json_updates_without_peer_compression(&VersionVector::default(), &other.oplog_vv())
        .unwrap();
    // Both orders: the future dep must not hide the trimmed dep.
    for deps in [
        vec![ID::new(1, 0), ID::new(9, 0)],
        vec![ID::new(9, 0), ID::new(1, 0)],
    ] {
        json.changes[0].deps = deps;
        let doc = LoroDoc::new();
        doc.import(&snapshot).unwrap();
        assert_eq!(
            doc.import_json_updates(json.clone()).unwrap_err(),
            LoroError::ImportUpdatesThatDependsOnOutdatedVersion
        );
        assert!(doc.export_pending_updates().unwrap().is_none());
        assert_eq!(doc.get_deep_value(), source.get_deep_value());
    }
}

#[test]
fn snapshot_import_preserves_pending_instead_of_trimming_its_deps() {
    let source = LoroDoc::new();
    source.set_peer_id(1).unwrap();
    source.get_map("m").insert("base", 1).unwrap();
    source.commit();
    let base = source.oplog_vv();
    let base_update = source.export(ExportMode::all_updates()).unwrap();
    let fork = source.fork();
    fork.set_peer_id(2).unwrap();
    fork.get_map("m").insert("concurrent", 2).unwrap();
    fork.commit();
    let pending = fork.export(ExportMode::updates(&base)).unwrap();
    source.get_map("m").insert("root", 3).unwrap();
    source.commit();
    let snapshot = source
        .export(ExportMode::shallow_snapshot(&source.oplog_frontiers()))
        .unwrap();
    let target = LoroDoc::new();
    assert!(target.import(&pending).unwrap().pending.is_some());
    target.import(&snapshot).unwrap();
    // Current main deliberately imports history only when pending changes exist.
    assert!(target.shallow_since_frontiers().is_empty());
    target.import(&base_update).unwrap();
    source.import(&pending).unwrap();
    assert_eq!(target.get_deep_value(), source.get_deep_value());
    assert_eq!(target.state_frontiers(), target.oplog_frontiers());
    assert!(target.export_pending_updates().unwrap().is_none());
}

#[test]
fn binary_update_mixing_root_and_trimmed_peer_is_rejected() {
    let base = LoroDoc::new();
    base.set_peer_id(1).unwrap();
    base.get_map("m").insert("base", 1).unwrap();
    base.commit();
    let source = LoroDoc::new();
    source.set_peer_id(2).unwrap();
    source
        .import(&base.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    source.get_map("m").insert("root", 2).unwrap();
    source.commit();
    let snapshot = source
        .export(ExportMode::shallow_snapshot(&source.oplog_frontiers()))
        .unwrap();
    let other = LoroDoc::new();
    other.set_peer_id(7).unwrap();
    other.get_map("m").insert("other", 3).unwrap();
    other.commit();
    let mut json = other
        .export_json_updates_without_peer_compression(&VersionVector::default(), &other.oplog_vv())
        .unwrap();
    json.changes[0].deps = vec![ID::new(2, 0), ID::new(1, 0)];
    // A full-history carrier can resolve both deps and encode this redundant
    // frontier unchanged. The shallow receiver has no node for peer 1's dep.
    let carrier = source.fork();
    carrier.import_json_updates(json).unwrap();
    let binary = carrier
        .export(ExportMode::updates(&source.oplog_vv()))
        .unwrap();
    let empty = LoroDoc::new().export(ExportMode::all_updates()).unwrap();
    for batch in [false, true] {
        let target = LoroDoc::new();
        target.import(&snapshot).unwrap();
        let result = if batch {
            target.import_batch(&[binary.clone(), empty.clone()])
        } else {
            target.import(&binary)
        };
        assert_eq!(
            result.unwrap_err(),
            LoroError::ImportUpdatesThatDependsOnOutdatedVersion
        );
        assert_eq!(target.oplog_vv(), source.oplog_vv());
        assert_eq!(target.get_deep_value(), source.get_deep_value());
        assert!(!target.is_detached());
        assert!(target.export_pending_updates().unwrap().is_none());
    }
}
