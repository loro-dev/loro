//! Checkout, version diff, fork and undo over text ops longer than one
//! id-to-cursor fragment (256 ids), checked against states recorded while the
//! history was written. A checkout across such an op updates one rope leaf
//! once per fragment; those updates are coalesced, and this is the coverage
//! for that path: the fuzz targets only insert short strings.
//! See `context/large-text-linear-time.md`.

use loro::{ExportMode, Frontiers, LoroDoc, UndoManager};
use rand::{rngs::StdRng, Rng, SeedableRng};

fn gen_text(rng: &mut StdRng, chars: usize) -> String {
    const WORDS: &[&str] = &["loro", "note", "é", "漢字", "😀", "\n", "**bold**", "a"];
    let mut s = String::new();
    let mut n = 0;
    while n < chars {
        for c in WORDS[rng.gen_range(0..WORDS.len())].chars().chain([' ']) {
            if n < chars {
                s.push(c);
                n += 1;
            }
        }
    }
    s
}

/// One random edit; most inserts and deletes span several fragments.
fn edit(doc: &LoroDoc, rng: &mut StdRng) {
    let text = doc.get_text("note");
    let len = text.len_unicode();
    match rng.gen_range(0..10) {
        0..=3 => {
            let n = rng.gen_range(1..1500);
            let pos = rng.gen_range(0..=len);
            text.insert(pos, &gen_text(rng, n)).unwrap();
        }
        4 => {
            let pos = rng.gen_range(0..=len);
            text.insert(pos, &gen_text(rng, 5)).unwrap();
        }
        5..=6 if len > 0 => {
            let pos = rng.gen_range(0..len);
            let n = rng.gen_range(1..=(len - pos).min(1200));
            text.delete(pos, n).unwrap();
        }
        7 if len > 0 => {
            // Backspace run: single deletes going left.
            let end = rng.gen_range(1..=len);
            let n = rng.gen_range(1..=end.min(400));
            for i in 0..n {
                text.delete(end - 1 - i, 1).unwrap();
            }
        }
        8 if len > 1 => {
            let start = rng.gen_range(0..len - 1);
            let end = rng.gen_range(start + 1..=len.min(start + 900));
            text.mark(start..end, "bold", true).unwrap();
        }
        _ => {
            text.insert(len, "tail").unwrap();
        }
    }
    doc.commit();
}

fn rich(doc: &LoroDoc) -> String {
    format!("{:?}", doc.get_text("note").to_delta())
}

#[test]
fn time_travel_over_multi_fragment_ops_matches_recorded_states() {
    for seed in 0..3 {
        let mut rng = StdRng::seed_from_u64(seed);
        let peers: Vec<LoroDoc> = (0..3)
            .map(|i| {
                let doc = LoroDoc::new();
                doc.set_peer_id(i + 1).unwrap();
                doc
            })
            .collect();
        // The observer imports everything and records each version it sees.
        let observer = LoroDoc::new();
        observer.set_peer_id(100).unwrap();
        let mut recorded: Vec<(Frontiers, String, String)> =
            vec![(observer.oplog_frontiers(), String::new(), rich(&observer))];
        for _ in 0..40 {
            let p = rng.gen_range(0..peers.len());
            edit(&peers[p], &mut rng);
            if rng.gen_bool(0.4) {
                let q = rng.gen_range(0..peers.len());
                let updates = peers[p]
                    .export(ExportMode::updates(&peers[q].oplog_vv()))
                    .unwrap();
                peers[q].import(&updates).unwrap();
            }
            let updates = peers[p]
                .export(ExportMode::updates(&observer.oplog_vv()))
                .unwrap();
            if rng.gen_bool(0.5) {
                observer.import(&updates).unwrap();
            } else {
                observer.import_batch(&[updates]).unwrap();
            }
            recorded.push((
                observer.oplog_frontiers(),
                observer.get_text("note").to_string(),
                rich(&observer),
            ));
        }

        for _ in 0..8 {
            let (a, text_a, rich_a) = &recorded[rng.gen_range(0..recorded.len())];
            let (b, text_b, rich_b) = &recorded[rng.gen_range(0..recorded.len())];
            observer.checkout(a).unwrap();
            assert_eq!(
                &observer.get_text("note").to_string(),
                text_a,
                "seed {seed}"
            );
            assert_eq!(&rich(&observer), rich_a, "seed {seed}");
            // Straight from one old version to another, not through the latest.
            observer.checkout(b).unwrap();
            assert_eq!(
                &observer.get_text("note").to_string(),
                text_b,
                "seed {seed}"
            );
            assert_eq!(&rich(&observer), rich_b, "seed {seed}");
            observer.try_checkout_to_latest().unwrap();

            let fork = observer.fork_at(a).unwrap();
            assert_eq!(&fork.get_text("note").to_string(), text_a, "seed {seed}");
            fork.apply_diff(observer.diff(a, b).unwrap()).unwrap();
            assert_eq!(&fork.get_text("note").to_string(), text_b, "seed {seed}");
        }
        let (_, text, _) = recorded.last().unwrap();
        assert_eq!(&observer.get_text("note").to_string(), text, "seed {seed}");
    }
}

#[test]
fn undo_and_redo_over_multi_fragment_ops_restore_recorded_text() {
    for seed in 0..3 {
        let mut rng = StdRng::seed_from_u64(1000 + seed);
        let doc = LoroDoc::new();
        doc.set_peer_id(1).unwrap();
        let remote = LoroDoc::new();
        remote.set_peer_id(2).unwrap();
        let mut undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);
        let mut states = vec![doc.get_text("note").to_string()];
        for _ in 0..25 {
            let before = doc.oplog_frontiers();
            edit(&doc, &mut rng);
            // A mark over text that already has it makes no op and no undo step.
            if doc.oplog_frontiers() != before {
                states.push(doc.get_text("note").to_string());
            }
        }
        for expected in states.iter().rev().skip(1) {
            assert!(undo.undo().unwrap(), "seed {seed}");
            assert_eq!(&doc.get_text("note").to_string(), expected, "seed {seed}");
        }
        for expected in states.iter().skip(1) {
            assert!(undo.redo().unwrap(), "seed {seed}");
            assert_eq!(&doc.get_text("note").to_string(), expected, "seed {seed}");
        }

        // Undo with a remote edit in between still converges with the remote.
        remote
            .import(&doc.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        let n = gen_text(&mut rng, 700);
        remote.get_text("note").insert(0, &n).unwrap();
        remote.commit();
        doc.import(&remote.export(ExportMode::updates(&doc.oplog_vv())).unwrap())
            .unwrap();
        for _ in 0..10 {
            assert!(undo.undo().unwrap(), "seed {seed}");
        }
        remote
            .import(&doc.export(ExportMode::updates(&remote.oplog_vv())).unwrap())
            .unwrap();
        assert_eq!(
            doc.get_text("note").to_string(),
            remote.get_text("note").to_string(),
            "seed {seed}"
        );
        assert!(doc.get_text("note").to_string().starts_with(&n));
    }
}
