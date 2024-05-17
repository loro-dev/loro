use std::sync::{Arc, Mutex};

use either::Either;
use fxhash::FxHashMap;
use loro_common::{
    ContainerID, Counter, CounterSpan, HasCounterSpan, HasIdSpan, IdSpan, LoroResult, PeerID,
};
use tracing::{debug_span, instrument, trace};

use crate::{
    container::idx::ContainerIdx,
    event::{Diff, EventTriggerKind},
    version::Frontiers,
    DocDiff, LoroDoc,
};

#[derive(Debug, Clone)]
pub struct DiffBatch(pub(crate) FxHashMap<ContainerID, Diff>);

#[derive(Debug)]
struct Generation(usize);

#[derive(Debug)]
pub struct UndoManager {
    peer: PeerID,
    latest_counter: Counter,
    undo_stack: Vec<(CounterSpan, Generation)>,
    redo_stack: Vec<(CounterSpan, Generation)>,
    container_remap: FxHashMap<ContainerID, ContainerID>,
    remote_diffs: Arc<Mutex<Vec<DiffBatch>>>,
}

impl DiffBatch {
    pub fn new(diff: Vec<DocDiff>) -> Self {
        let mut map: FxHashMap<ContainerID, Diff> = Default::default();
        for d in diff.into_iter() {
            for item in d.diff.into_iter() {
                let old = map.insert(item.id.clone(), item.diff);
                assert!(old.is_none());
            }
        }

        Self(map)
    }

    pub fn compose(&mut self, other: &Self) {
        if other.0.is_empty() {
            return;
        }

        for (idx, diff) in self.0.iter_mut() {
            if let Some(b_diff) = other.0.get(idx) {
                diff.compose_ref(b_diff);
            }
        }
    }

    pub fn transform(&mut self, other: &Self, left_priority: bool) {
        if other.0.is_empty() {
            return;
        }

        for (idx, diff) in self.0.iter_mut() {
            if let Some(b_diff) = other.0.get(idx) {
                diff.transform(b_diff, left_priority);
            }
        }
    }

    pub fn filter(&mut self, container_filter: &[ContainerIdx]) {
        unimplemented!()
    }
}

fn get_counter_end(doc: &LoroDoc, peer: PeerID) -> Counter {
    doc.oplog()
        .lock()
        .unwrap()
        .get_peer_changes(peer)
        .and_then(|x| x.last().map(|x| x.ctr_end()))
        .unwrap_or(0)
}

impl UndoManager {
    pub fn new(peer: PeerID, doc: &LoroDoc) -> Self {
        let remote_diff = Arc::new(Mutex::new(vec![DiffBatch::new(Default::default())]));
        let remote_diff_clone = remote_diff.clone();
        doc.subscribe_root(Arc::new(move |event| {
            if matches!(event.event_meta.by, EventTriggerKind::Import) {
                let mut remote_diffs = remote_diff_clone.lock().unwrap();
                assert!(!remote_diffs.is_empty());
                for remote_diff in remote_diffs.iter_mut() {
                    for e in event.events {
                        if let Some(d) = remote_diff.0.get_mut(&e.id) {
                            d.compose_ref(&e.diff);
                        } else {
                            remote_diff.0.insert(e.id.clone(), e.diff.clone());
                        }
                    }
                }
            }
        }));
        UndoManager {
            peer,
            latest_counter: get_counter_end(doc, peer),
            undo_stack: vec![],
            redo_stack: vec![],
            container_remap: Default::default(),
            remote_diffs: remote_diff,
        }
    }

    fn get_next_gen(&mut self) -> Generation {
        let mut remote_diffs = self.remote_diffs.lock().unwrap();
        if !remote_diffs.last().unwrap().0.is_empty() {
            remote_diffs.push(DiffBatch::new(Default::default()));
        }
        Generation(remote_diffs.len() - 1)
    }

    pub fn record_new_checkpoint(&mut self, doc: &LoroDoc) {
        doc.commit_then_renew();
        let counter = get_counter_end(doc, self.peer);
        if counter == self.latest_counter {
            return;
        }

        let gen = self.get_next_gen();
        assert!(self.latest_counter < counter);
        let span = CounterSpan::new(self.latest_counter, counter);
        self.latest_counter = counter;
        self.undo_stack.push((span, gen));
        self.redo_stack.clear();
    }

    #[instrument(skip_all)]
    pub fn undo(&mut self, doc: &LoroDoc) -> LoroResult<()> {
        self.record_new_checkpoint(doc);
        let end_counter = get_counter_end(doc, self.peer);

        if let Some((span, gen)) = self.undo_stack.pop() {
            trace!("Undo {:?}", span);
            {
                let diffs = self.remote_diffs.lock().unwrap();
                // TODO: we can avoid this clone
                let e = diffs[gen.0].clone();
                doc.undo(
                    IdSpan {
                        peer: self.peer,
                        counter: span,
                        // counter: CounterSpan::new(span.start, end_counter),
                    },
                    &mut self.container_remap,
                    Some(&e),
                )?;
            }
            let new_counter = get_counter_end(doc, self.peer);
            if end_counter != new_counter {
                let gen = self.get_next_gen();
                self.redo_stack
                    .push((CounterSpan::new(end_counter, new_counter), gen));
            }
            self.latest_counter = new_counter;
        }

        Ok(())
    }

    #[instrument(skip_all)]
    pub fn redo(&mut self, doc: &LoroDoc) -> LoroResult<()> {
        self.record_new_checkpoint(doc);
        let end_counter = get_counter_end(doc, self.peer);
        if let Some((span, gen)) = self.redo_stack.pop() {
            let e = self.remote_diffs.lock().unwrap()[gen.0].clone();
            doc.undo(
                IdSpan {
                    peer: self.peer,
                    counter: span,
                    // counter: CounterSpan::new(span.start, end_counter),
                },
                &mut self.container_remap,
                Some(&e),
            )?;
            let new_counter = get_counter_end(doc, self.peer);
            if end_counter != new_counter {
                let gen = self.get_next_gen();
                self.undo_stack
                    .push((CounterSpan::new(end_counter, new_counter), gen));
            }
            self.latest_counter = new_counter;
        }

        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

/// Undo the given spans of operations.
///
/// # Parameters
///
/// - `spans`: A vector of tuples where each tuple contains an `IdSpan` and its associated `Frontiers`.
///   - `IdSpan`: Represents a span of operations identified by an ID.
///   - `Frontiers`: Represents the deps of the given id_span
/// - `latest_frontiers`: The latest frontiers of the document
/// - `calc_diff`: A closure that takes two `Frontiers` and calculates the difference between them, returning a `DiffBatch`.
///
/// # Returns
///
/// - `DiffBatch`: Applying this batch on the `latest_frontiers` will undo the ops in the given spans.
pub(crate) fn undo(
    spans: Vec<(IdSpan, Frontiers)>,
    last_frontiers_or_last_bi: Either<&Frontiers, &DiffBatch>,
    calc_diff: impl Fn(&Frontiers, &Frontiers) -> DiffBatch,
) -> DiffBatch {
    // The process of performing undo is:
    //
    // 0. Split the span into a series of continuous spans. There is no external dep within each continuous span.
    //
    // For each continuous span_i:
    //
    // 1. a. Calculate the event of checkout from id_span.last to id_span.deps, call it Ai. It undo the ops in the current span.
    //    b. Calculate A'i = Ai + T(Ci-1, Ai) if i > 0, otherwise A'i = Ai.
    //       NOTE: A'i can undo the ops in the current span and the previous spans, if it's applied on the id_span.last version.
    // 2. Calculate the event of checkout from id_span.last to [the next span's last id] or [the latest version], call it Bi.
    // 3. Transform event A'i based on Bi, call it Ci
    // 4. If span_i is the last span, apply Ci to the current state.

    // -------------------------------------------------------
    // 0. Split the span into a series of continuous spans
    // -------------------------------------------------------

    let mut last_ci: Option<DiffBatch> = None;
    for i in 0..spans.len() {
        debug_span!("Undo", ?i, "Undo span {:?}", &spans[i]).in_scope(|| {
            let (this_id_span, this_deps) = &spans[i];
            // ---------------------------------------
            // 1.a Calc event A_i
            // ---------------------------------------
            let mut event_a_i = debug_span!("1. Calc event A_i").in_scope(|| {
                // Checkout to the last id of the id_span
                calc_diff(&this_id_span.id_last().into(), this_deps)
            });

            trace!("Event A_i {:#?}", &event_a_i.0);
            // ---------------------------------------
            // 2. Calc event B_i
            // ---------------------------------------
            let mut stack_diff_batch = None;
            let event_b_i = debug_span!("2. Calc event B_i").in_scope(|| {
                let next = if i + 1 < spans.len() {
                    spans[i + 1].0.id_last().into()
                } else {
                    match last_frontiers_or_last_bi {
                        Either::Left(last_frontiers) => last_frontiers.clone(),
                        Either::Right(right) => return right,
                    }
                };

                stack_diff_batch = Some(calc_diff(&this_id_span.id_last().into(), &next));
                stack_diff_batch.as_ref().unwrap()
            });
            trace!("Event B_i {:#?}", &event_b_i.0);

            // event_a_prime can undo the ops in the current span and the previous spans
            let mut event_a_prime = if let Some(mut last_ci) = last_ci.take() {
                // ------------------------------------------------------------------------------
                // 1.b Transform and apply Ci-1 based on Ai, call it A'i
                // ------------------------------------------------------------------------------
                trace!("last_ci {:#?}", last_ci.0);
                trace!("event_a_i {:#?}", &event_a_i.0);
                last_ci.transform(&event_a_i, true);
                trace!("transformed last_ci {:#?}", last_ci.0);
                event_a_i.compose(&last_ci);
                event_a_i
            } else {
                event_a_i
            };
            trace!("Event A'_i {:#?}", &event_a_prime.0);

            // --------------------------------------------------
            // 3. Transform event A'_i based on B_i, call it C_i
            // --------------------------------------------------
            event_a_prime.transform(&event_b_i, true);
            let c_i = event_a_prime;
            trace!("Event C_i {:#?}", &c_i.0);
            last_ci = Some(c_i);
        });
    }

    last_ci.unwrap()
}
