use criterion::{criterion_group, criterion_main, Criterion};

#[cfg(feature = "test_utils")]
mod run {
    use super::*;
    use loro_internal::{
        handler::{HandlerTrait, UpdateOptions},
        LoroDoc, UndoManager, UndoScope,
    };

    /// Number of commits to record per benchmark iteration. Small enough to keep
    /// runs fast, large enough to amortize per-iteration timer noise so we can
    /// see sub-microsecond per-commit deltas between configurations.
    const N_COMMITS: usize = 1_000;

    fn one_text_edit(loro: &LoroDoc, value: &str) {
        let text = loro.get_text("text");
        text.update(value, UpdateOptions::default()).unwrap();
        loro.commit_then_renew();
    }

    /// Record-time cost: build a doc, attach an UndoManager (in three different
    /// configurations), and measure the time to record `N_COMMITS` local commits.
    /// This is the hot path the subscription callback sits on.
    pub fn record_local_commits(c: &mut Criterion) {
        let mut g = c.benchmark_group("undo/record_local_commits");
        g.sample_size(50);

        // Baseline: no UndoManager attached at all. Establishes the cost of just
        // committing N edits, so we can isolate the manager's overhead.
        g.bench_function("no_manager", |b| {
            b.iter(|| {
                let loro = LoroDoc::default();
                for i in 0..N_COMMITS {
                    one_text_edit(&loro, &format!("v{}", i));
                }
            });
        });

        // Default: UndoManager with UndoScope::Doc (current behavior, our changes
        // must not regress this). Compares directly against the same code on main.
        g.bench_function("undo_manager_default_scope", |b| {
            b.iter(|| {
                let loro = LoroDoc::default();
                let _undo = UndoManager::new(&loro);
                for i in 0..N_COMMITS {
                    one_text_edit(&loro, &format!("v{}", i));
                }
            });
        });

        // Scoped: UndoManager with UndoScope::Containers([text_id]). Quantifies
        // the cost users opt into when they enable scope. Single-container scope
        // is the smallest possible set; larger sets only affect FxHashSet lookup
        // (constant-time average).
        g.bench_function("undo_manager_scoped_one_container", |b| {
            b.iter(|| {
                let loro = LoroDoc::default();
                let text = loro.get_text("text");
                let _undo = UndoManager::new(&loro)
                    .with_scope(UndoScope::containers([text.id()]));
                for i in 0..N_COMMITS {
                    one_text_edit(&loro, &format!("v{}", i));
                }
            });
        });
    }

    /// Mixed-scope workload: alternate edits between an in-scope and an
    /// out-of-scope container. Out-of-scope commits hit the
    /// `compose_remote_event` branch instead of `record_checkpoint`. This
    /// stresses the path the scope feature actually exists for.
    pub fn record_mixed_scope(c: &mut Criterion) {
        let mut g = c.benchmark_group("undo/record_mixed_scope");
        g.sample_size(50);

        // Doc-wide for reference: every commit is recorded.
        g.bench_function("doc_scope_all_recorded", |b| {
            b.iter(|| {
                let loro = LoroDoc::default();
                let text_a = loro.get_text("a");
                let text_b = loro.get_text("b");
                let _undo = UndoManager::new(&loro);
                for i in 0..(N_COMMITS / 2) {
                    text_a
                        .update(&format!("a{}", i), UpdateOptions::default())
                        .unwrap();
                    loro.commit_then_renew();
                    text_b
                        .update(&format!("b{}", i), UpdateOptions::default())
                        .unwrap();
                    loro.commit_then_renew();
                }
            });
        });

        // Container scope = {a}: half the commits are filtered to the
        // compose-as-remote branch.
        g.bench_function("scoped_a_half_filtered", |b| {
            b.iter(|| {
                let loro = LoroDoc::default();
                let text_a = loro.get_text("a");
                let text_b = loro.get_text("b");
                let _undo = UndoManager::new(&loro)
                    .with_scope(UndoScope::containers([text_a.id()]));
                for i in 0..(N_COMMITS / 2) {
                    text_a
                        .update(&format!("a{}", i), UpdateOptions::default())
                        .unwrap();
                    loro.commit_then_renew();
                    text_b
                        .update(&format!("b{}", i), UpdateOptions::default())
                        .unwrap();
                    loro.commit_then_renew();
                }
            });
        });
    }

    /// Replay-time cost: build a doc with N recorded commits, then measure the
    /// time to undo every commit followed by redoing every commit. Exercises
    /// `undo_internal_with_scope` end-to-end including the optional mask block.
    pub fn undo_redo_all(c: &mut Criterion) {
        let mut g = c.benchmark_group("undo/replay_all");
        g.sample_size(20);

        g.bench_function("doc_scope", |b| {
            b.iter_batched(
                || {
                    let loro = LoroDoc::default();
                    let undo = UndoManager::new(&loro);
                    for i in 0..N_COMMITS {
                        one_text_edit(&loro, &format!("v{}", i));
                    }
                    (loro, undo)
                },
                |(_loro, undo)| {
                    while undo.can_undo() {
                        undo.undo().unwrap();
                    }
                    while undo.can_redo() {
                        undo.redo().unwrap();
                    }
                },
                criterion::BatchSize::SmallInput,
            );
        });

        g.bench_function("scoped_one_container", |b| {
            b.iter_batched(
                || {
                    let loro = LoroDoc::default();
                    let text = loro.get_text("text");
                    let undo = UndoManager::new(&loro)
                        .with_scope(UndoScope::containers([text.id()]));
                    for i in 0..N_COMMITS {
                        one_text_edit(&loro, &format!("v{}", i));
                    }
                    (loro, undo)
                },
                |(_loro, undo)| {
                    while undo.can_undo() {
                        undo.undo().unwrap();
                    }
                    while undo.can_redo() {
                        undo.redo().unwrap();
                    }
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
}

pub fn dumb(_c: &mut Criterion) {}

#[cfg(feature = "test_utils")]
criterion_group!(
    benches,
    run::record_local_commits,
    run::record_mixed_scope,
    run::undo_redo_all,
);
#[cfg(not(feature = "test_utils"))]
criterion_group!(benches, dumb);
criterion_main!(benches);
