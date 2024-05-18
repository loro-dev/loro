use std::sync::{Arc, Mutex};

use either::Either;
use fxhash::FxHashMap;
use loro_common::{
    ContainerID, Counter, CounterSpan, HasCounterSpan, HasIdSpan, IdSpan, LoroResult, PeerID,
};
use tracing::{debug_span, instrument, trace};

use crate::{
    event::{Diff, EventTriggerKind},
    version::Frontiers,
    DocDiff, LoroDoc,
};

#[derive(Debug, Clone, Default)]
pub struct DiffBatch(pub(crate) FxHashMap<ContainerID, Diff>);

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
}

#[derive(Debug)]
struct Generation(usize);

/// UndoManager is responsible for managing undo/redo from the current peer's perspective.
///
/// Undo/local is local: it cannot be used to undone the changes made by other peers.
/// If you want to undo changes made by other peers, you may need to use the time travel feature.
///
/// PeerID cannot be changed during the lifetime of the UndoManager
#[derive(Debug)]
pub struct UndoManager {
    peer: PeerID,
    container_remap: FxHashMap<ContainerID, ContainerID>,
    inner: Arc<Mutex<UndoManagerInner>>,
}

#[derive(Debug)]
struct UndoManagerInner {
    latest_counter: Counter,
    undo_stack: Vec<(CounterSpan, Generation)>,
    redo_stack: Vec<(CounterSpan, Generation)>,
    remote_diffs: Vec<DiffBatch>,
    processing_undo: bool,
}

impl UndoManagerInner {
    fn new(last_counter: Counter) -> Self {
        UndoManagerInner {
            latest_counter: last_counter,
            undo_stack: Default::default(),
            redo_stack: Default::default(),
            remote_diffs: vec![DiffBatch::default()],
            processing_undo: false,
        }
    }

    fn get_next_gen(&mut self) -> Generation {
        if !self.remote_diffs.last().unwrap().0.is_empty() {
            self.remote_diffs.push(DiffBatch::new(Default::default()));
        }
        Generation(self.remote_diffs.len() - 1)
    }

    fn record_checkpoint(&mut self, latest_counter: Counter) {
        if latest_counter == self.latest_counter {
            return;
        }

        trace!("record_checkpoint {}", latest_counter);
        trace!("undo_stack {:#?}", &self.undo_stack);

        let gen = self.get_next_gen();
        assert!(self.latest_counter < latest_counter);
        let span = CounterSpan::new(self.latest_counter, latest_counter);
        self.latest_counter = latest_counter;
        self.undo_stack.push((span, gen));
        trace!("undo_stack {:#?}", &self.undo_stack);
        self.redo_stack.clear();
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
    pub fn new(doc: &LoroDoc) -> Self {
        let peer = doc.peer_id();
        let inner = Arc::new(Mutex::new(UndoManagerInner::new(get_counter_end(
            doc, peer,
        ))));
        let inner_clone = inner.clone();
        doc.subscribe_root(Arc::new(move |event| match event.event_meta.by {
            EventTriggerKind::Local => {
                let mut inner = inner_clone.try_lock().unwrap();
                if !inner.processing_undo {
                    if let Some(id) = event.event_meta.to.iter().find(|x| x.peer == peer) {
                        inner.record_checkpoint(id.counter + 1);
                    }
                }
            }
            EventTriggerKind::Import => {
                let mut inner = inner_clone.try_lock().unwrap();
                assert!(!inner.remote_diffs.is_empty());
                for remote_diff in inner.remote_diffs.iter_mut() {
                    for e in event.events {
                        if let Some(d) = remote_diff.0.get_mut(&e.id) {
                            d.compose_ref(&e.diff);
                        } else {
                            remote_diff.0.insert(e.id.clone(), e.diff.clone());
                        }
                    }
                }
            }
            EventTriggerKind::Checkout => {}
        }));

        UndoManager {
            peer,
            container_remap: Default::default(),
            inner,
        }
    }

    pub fn record_new_checkpoint(&mut self, doc: &LoroDoc) {
        if doc.peer_id() != self.peer {
            panic!("PeerID cannot be changed during the lifetime of the UndoManager")
        }

        doc.commit_then_renew();
        let counter = get_counter_end(doc, self.peer);
        self.inner.lock().unwrap().record_checkpoint(counter);
    }

    #[instrument(skip_all)]
    pub fn undo(&mut self, doc: &LoroDoc) -> LoroResult<()> {
        self.record_new_checkpoint(doc);
        let end_counter = get_counter_end(doc, self.peer);
        let mut top = {
            let mut inner = self.inner.lock().unwrap();
            inner.processing_undo = true;
            inner.undo_stack.pop()
        };
        while let Some((span, gen)) = top {
            trace!("Undo {:?}", span);
            {
                // TODO: we can avoid this clone
                let e = self.inner.lock().unwrap().remote_diffs[gen.0].clone();
                doc.undo_internal(
                    IdSpan {
                        peer: self.peer,
                        counter: span,
                    },
                    &mut self.container_remap,
                    Some(&e),
                )?;
            }
            let new_counter = get_counter_end(doc, self.peer);
            if end_counter != new_counter {
                let mut inner = self.inner.lock().unwrap();
                let gen = inner.get_next_gen();
                inner
                    .redo_stack
                    .push((CounterSpan::new(end_counter, new_counter), gen));
                inner.latest_counter = new_counter;
                break;
            } else {
                // continue to pop the undo item as this undo is a no-op
                top = self.inner.lock().unwrap().undo_stack.pop();
                continue;
            }
        }

        self.inner.lock().unwrap().processing_undo = false;
        Ok(())
    }

    #[instrument(skip_all)]
    pub fn redo(&mut self, doc: &LoroDoc) -> LoroResult<()> {
        self.record_new_checkpoint(doc);
        let end_counter = get_counter_end(doc, self.peer);

        let mut top = {
            let mut inner = self.inner.lock().unwrap();
            inner.processing_undo = true;
            inner.redo_stack.pop()
        };
        while let Some((span, gen)) = top {
            let e = self.inner.lock().unwrap().remote_diffs[gen.0].clone();
            {
                doc.undo_internal(
                    IdSpan {
                        peer: self.peer,
                        counter: span,
                    },
                    &mut self.container_remap,
                    Some(&e),
                )?;
            }
            let new_counter = get_counter_end(doc, self.peer);
            if end_counter != new_counter {
                let mut inner = self.inner.lock().unwrap();
                let gen = inner.get_next_gen();

                inner
                    .undo_stack
                    .push((CounterSpan::new(end_counter, new_counter), gen));
                inner.latest_counter = new_counter;
                break;
            } else {
                // continue to pop the redo item as this redo is a no-op
                top = self.inner.lock().unwrap().redo_stack.pop();
                continue;
            }
        }

        self.inner.lock().unwrap().processing_undo = false;
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.inner.lock().unwrap().undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.inner.lock().unwrap().redo_stack.is_empty()
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
                // left_prior is false because event_a_i happens first
                last_ci.transform(&event_a_i, false);
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
            // left_prior is false because event_b_i happens first
            event_a_prime.transform(event_b_i, false);
            let c_i = event_a_prime;
            trace!("Event C_i {:#?}", &c_i.0);
            last_ci = Some(c_i);
        });
    }

    last_ci.unwrap()
}
