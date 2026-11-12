//! Regression for loro-dev/loro#1117: skipped concurrent moves must stay skipped
//! when resolving historical versions.
use loro::{
    ExportMode, Frontiers, LoroDoc, TreeID,
    TreeParentId::{Node, Root},
};

fn scenario() -> (LoroDoc, LoroDoc, Frontiers, Frontiers, TreeID, TreeID) {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    let ta = a.get_tree("tree");
    let tb = b.get_tree("tree");
    let n0 = ta.create(Root).unwrap();
    let n1 = ta.create(Root).unwrap();
    a.commit();
    let base = a.state_frontiers();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    ta.mov(n1, Node(n0)).unwrap();
    a.commit();
    let a_so_far = a.export(ExportMode::all_updates()).unwrap();
    tb.mov(n0, Root).unwrap();
    b.commit();
    tb.mov(n0, Node(n1)).unwrap();
    b.commit();
    b.import(&a_so_far).unwrap();
    let target = b.state_frontiers();
    ta.mov(n1, Root).unwrap();
    a.commit();
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    (a, b, base, target, n0, n1)
}

struct Fixture {
    doc: LoroDoc,
    base: Frontiers,
    target: Frontiers,
    latest: Frontiers,
    expected: loro::LoroValue,
    latest_value: loro::LoroValue,
}

impl Fixture {
    fn new() -> Self {
        let (doc, b, base, target, n0, n1) = scenario();
        let latest = doc.state_frontiers();
        let replay_at = |version: &Frontiers| {
            let replay = LoroDoc::new();
            let vv = doc.frontiers_to_vv(version).unwrap();
            replay
                .import(&doc.export(ExportMode::updates_till(&vv)).unwrap())
                .unwrap();
            replay
        };
        let replay = replay_at(&target);
        let expected = replay.get_deep_value();
        assert_eq!(b.get_deep_value(), expected);
        assert_eq!(replay.get_tree("tree").parent(n0), Some(Root));
        assert_eq!(replay.get_tree("tree").parent(n1), Some(Node(n0)));
        let latest_value = replay_at(&latest).get_deep_value();
        assert_eq!(doc.get_deep_value(), latest_value);
        Self {
            doc,
            base,
            target,
            latest,
            expected,
            latest_value,
        }
    }
}

// A stack overflow aborts rather than unwinds. Each probe gets its own child
// process, so a regression reports which API failed without killing the runner.
macro_rules! probe {
    ($name:ident, |$f:ident| $body:block) => {
        #[test]
        fn $name() {
            const CHILD: &str = "LORO_ISSUE_1117_CHILD";
            if std::env::var(CHILD).ok().as_deref() != Some(stringify!($name)) {
                let output = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", stringify!($name), "--nocapture"])
                    .env(CHILD, stringify!($name))
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "child status: {}\nstdout:\n{}\nstderr:\n{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                return;
            }
            let $f = Fixture::new();
            $body
        }
    };
}

probe!(fork_at_skipped_cycle_move, |f| {
    assert_eq!(
        f.doc.fork_at(&f.target).unwrap().get_deep_value(),
        f.expected
    );
});

probe!(checkout_skipped_cycle_move_round_trip, |f| {
    for subscribe in [false, true] {
        let doc = f.doc.fork();
        let _sub = subscribe.then(|| doc.subscribe_root(std::sync::Arc::new(|_| {})));
        for _ in 0..2 {
            doc.checkout(&f.target).unwrap();
            assert_eq!(doc.get_deep_value(), f.expected);
            doc.checkout(&f.latest).unwrap();
            assert_eq!(doc.get_deep_value(), f.latest_value);
        }
    }
});

probe!(diff_to_skipped_cycle_move, |f| {
    let diff = f.doc.diff(&f.latest, &f.target).unwrap();
    let doc = f.doc.fork();
    doc.apply_diff(diff).unwrap();
    assert_eq!(doc.get_deep_value(), f.expected);
    assert_eq!(f.doc.get_deep_value(), f.latest_value);
});

probe!(diff_from_skipped_cycle_move, |f| {
    let diff = f.doc.diff(&f.target, &f.latest).unwrap();
    let doc = LoroDoc::new();
    let vv = f.doc.frontiers_to_vv(&f.target).unwrap();
    doc.import(&f.doc.export(ExportMode::updates_till(&vv)).unwrap())
        .unwrap();
    doc.apply_diff(diff).unwrap();
    assert_eq!(doc.get_deep_value(), f.latest_value);
    assert_eq!(f.doc.get_deep_value(), f.latest_value);
});

probe!(revert_to_skipped_cycle_move, |f| {
    f.doc.revert_to(&f.target).unwrap();
    assert_eq!(f.doc.get_deep_value(), f.expected);
});

probe!(snapshot_at_skipped_cycle_move, |f| {
    let snapshot = LoroDoc::new();
    snapshot
        .import(&f.doc.export(ExportMode::snapshot_at(&f.target)).unwrap())
        .unwrap();
    assert_eq!(snapshot.get_deep_value(), f.expected);
    assert_eq!(f.doc.state_frontiers(), f.latest);
    assert!(!f.doc.is_detached());
    assert_eq!(f.doc.get_deep_value(), f.latest_value);
});

probe!(shallow_checkout_skipped_cycle_move_round_trip, |f| {
    for root in [&f.base, &f.target] {
        let shallow = LoroDoc::new();
        shallow
            .import(&f.doc.export(ExportMode::shallow_snapshot(root)).unwrap())
            .unwrap();
        for _ in 0..2 {
            shallow.checkout(&f.target).unwrap();
            assert_eq!(shallow.get_deep_value(), f.expected);
            shallow.checkout(&f.latest).unwrap();
            assert_eq!(shallow.get_deep_value(), f.latest_value);
        }
    }
});
