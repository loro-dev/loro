//! A richtext tracker rebuilt from empty history models every op of its
//! container by id, so the persistent diff calculator keeps it and only
//! replays the ops that arrived since, instead of rebuilding it from the
//! whole history on every retreat (every undo, every checkout back in time).

use crate::cursor::PosType;
use crate::diff_calc::FULL_TRACKER_REBUILD_COUNT;
use crate::loro::ExportMode;
use crate::undo::UndoManager;
use crate::version::Frontiers;
use crate::LoroDoc;

use super::exchange;

fn rebuilds() -> u64 {
    FULL_TRACKER_REBUILD_COUNT.with(|c| c.get())
}

/// Deterministic pseudo-random numbers, so a failure reproduces.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n.max(1)
    }
}

fn edit(doc: &LoroDoc, rng: &mut Rng) {
    let text = doc.get_text("text");
    let len = text.len_unicode();
    match rng.below(4) {
        0 if len > 2 => {
            let pos = rng.below(len - 1);
            text.delete_unicode(pos, 1 + rng.below((len - pos).min(3)))
                .unwrap();
        }
        1 if len > 2 => {
            let start = rng.below(len - 1);
            let end = start + 1 + rng.below(len - start - 1);
            text.mark(start, end, "bold", true.into(), PosType::Unicode)
                .unwrap();
        }
        _ => {
            text.insert_unicode(rng.below(len + 1), ["ab", "c", "def😀"][rng.below(3)])
                .unwrap();
        }
    }
    doc.commit_then_renew();
}

#[test]
fn repeated_undos_rebuild_the_text_tracker_once() {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    let mut rng = Rng(7);
    for _ in 0..300 {
        edit(&doc, &mut rng);
    }
    let undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);

    let before = rebuilds();
    for round in 0..10 {
        let expected = text.get_richtext_value();
        text.insert_unicode(round % 3, "typed").unwrap();
        doc.get_map("other").insert("round", round as i64).unwrap();
        doc.commit_then_renew();
        undo.undo().unwrap();
        assert_eq!(text.get_richtext_value(), expected, "undo {round}");
    }

    assert_eq!(
        rebuilds() - before,
        1,
        "only the first undo may rebuild the tracker from history"
    );
    doc.check_state_diff_calc_consistency_slow();
}

/// The text with its styles, which the deep value leaves out.
fn rich(doc: &LoroDoc) -> crate::LoroValue {
    doc.get_text("text").get_richtext_value()
}

/// Undo, redo and time travel on a doc that keeps receiving concurrent
/// remote edits reach the same states as a doc that has never computed a
/// diff, so whatever the persistent calculator kept is still exact.
#[test]
fn a_kept_text_tracker_matches_a_fresh_one() {
    for seed in 0..12 {
        let a = LoroDoc::new_auto_commit();
        a.set_peer_id(1).unwrap();
        let b = LoroDoc::new_auto_commit();
        b.set_peer_id(2).unwrap();
        let undo = UndoManager::new(&a);
        undo.set_merge_interval(0);
        let mut rng = Rng(seed);
        let mut seen: Vec<Frontiers> = Vec::new();

        for step in 0..120 {
            match rng.below(8) {
                0 | 1 => edit(&a, &mut rng),
                2 | 3 => edit(&b, &mut rng),
                4 => exchange(&a, &b),
                5 => {
                    undo.undo().unwrap();
                }
                6 => {
                    undo.redo().unwrap();
                }
                _ if !seen.is_empty() => {
                    let target = seen[rng.below(seen.len())].clone();
                    a.checkout(&target).unwrap();
                    let fresh =
                        LoroDoc::from_snapshot(&a.export(ExportMode::Snapshot).unwrap()).unwrap();
                    fresh.checkout(&target).unwrap();
                    assert_eq!(
                        (a.get_deep_value(), rich(&a)),
                        (fresh.get_deep_value(), rich(&fresh)),
                        "seed {seed} step {step}: checkout"
                    );
                    a.checkout_to_latest();
                }
                _ => {}
            }
            seen.push(a.oplog_frontiers());
        }

        exchange(&a, &b);
        assert_eq!(
            (a.get_deep_value(), rich(&a)),
            (b.get_deep_value(), rich(&b)),
            "seed {seed}"
        );
        a.check_state_diff_calc_consistency_slow();
    }
}
