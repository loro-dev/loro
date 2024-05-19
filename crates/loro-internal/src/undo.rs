use std::sync::{Arc, Mutex};

use either::Either;
use fxhash::FxHashMap;
use loro_common::{
    ContainerID, Counter, CounterSpan, HasCounterSpan, HasIdSpan, IdSpan, LoroError, LoroResult,
    PeerID,
};
use tracing::{debug_span, info_span, instrument, trace, trace_span};

use crate::{
    event::{Diff, EventTriggerKind},
    version::Frontiers,
    ContainerDiff, DocDiff, LoroDoc,
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

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

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
    undo_stack: Stack,
    redo_stack: Stack,
    processing_undo: bool,
}

#[derive(Debug)]
struct Stack {
    stack: Vec<(Vec<CounterSpan>, Arc<Mutex<DiffBatch>>)>,
}

impl Stack {
    pub fn new() -> Self {
        Stack {
            stack: vec![(vec![], Arc::new(Mutex::new(Default::default())))],
        }
    }

    pub fn pop(&mut self) -> Option<(CounterSpan, Arc<Mutex<DiffBatch>>)> {
        while self.stack.last().unwrap().0.is_empty() && self.stack.len() > 1 {
            let (_, diff) = self.stack.pop().unwrap();
            let diff = diff.try_lock().unwrap();
            if !diff.0.is_empty() {
                self.stack
                    .last_mut()
                    .unwrap()
                    .1
                    .try_lock()
                    .unwrap()
                    .compose(&diff);
            }
        }

        if self.stack.len() == 1 && self.stack.last().unwrap().0.is_empty() {
            self.stack.last_mut().unwrap().1.try_lock().unwrap().clear();
            return None;
        }

        let last = self.stack.last_mut().unwrap();
        last.0.pop().map(|x| (x, last.1.clone()))
    }

    pub fn push(&mut self, span: CounterSpan) {
        let last = self.stack.last_mut().unwrap();
        let mut last_remote_diff = last.1.try_lock().unwrap();
        if !last_remote_diff.0.is_empty() {
            if last.0.is_empty() {
                last.0.push(span);
                last_remote_diff.clear();
            } else {
                drop(last_remote_diff);
                self.stack
                    .push((vec![span], Arc::new(Mutex::new(DiffBatch::default()))));
            }
        } else {
            last.0.push(span);
        }
    }

    pub fn compose_remote_event(&mut self, diff: &[&ContainerDiff]) {
        if self.is_empty() {
            return;
        }

        let remote_diff = &mut self.stack.last_mut().unwrap().1;
        let mut remote_diff = remote_diff.try_lock().unwrap();
        for e in diff {
            if let Some(d) = remote_diff.0.get_mut(&e.id) {
                d.compose_ref(&e.diff);
            } else {
                remote_diff.0.insert(e.id.clone(), e.diff.clone());
            }
        }
    }

    pub fn transform_based_on_this_delta(&mut self, diff: &DiffBatch) {
        if self.is_empty() {
            return;
        }

        let remote_diff = &mut self.stack.last_mut().unwrap().1;
        remote_diff.try_lock().unwrap().transform(diff, true);
    }

    pub fn clear(&mut self) {
        self.stack = vec![(vec![], Default::default())];
    }

    fn is_empty(&self) -> bool {
        self.stack.len() == 1 && self.stack[0].0.is_empty()
    }
}

impl Default for Stack {
    fn default() -> Self {
        Stack::new()
    }
}

impl UndoManagerInner {
    fn new(last_counter: Counter) -> Self {
        UndoManagerInner {
            latest_counter: last_counter,
            undo_stack: Default::default(),
            redo_stack: Default::default(),
            processing_undo: false,
        }
    }

    fn record_checkpoint(&mut self, latest_counter: Counter) {
        if latest_counter == self.latest_counter {
            return;
        }

        assert!(self.latest_counter < latest_counter);
        let span = CounterSpan::new(self.latest_counter, latest_counter);
        self.latest_counter = latest_counter;
        self.undo_stack.push(span);
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
                let Ok(mut inner) = inner_clone.try_lock() else {
                    return;
                };
                if !inner.processing_undo {
                    if let Some(id) = event.event_meta.to.iter().find(|x| x.peer == peer) {
                        inner.record_checkpoint(id.counter + 1);
                    }
                }
            }
            EventTriggerKind::Import => {
                let mut inner = inner_clone.try_lock().unwrap();
                inner.undo_stack.compose_remote_event(event.events);
                inner.redo_stack.compose_remote_event(event.events);
            }
            EventTriggerKind::Checkout => {}
        }));

        UndoManager {
            peer,
            container_remap: Default::default(),
            inner,
        }
    }

    pub fn record_new_checkpoint(&mut self, doc: &LoroDoc) -> LoroResult<()> {
        if doc.peer_id() != self.peer {
            return Err(LoroError::UndoWithDifferentPeerId {
                expected: self.peer,
                actual: doc.peer_id(),
            });
        }

        doc.commit_then_renew();
        let counter = get_counter_end(doc, self.peer);
        self.inner.try_lock().unwrap().record_checkpoint(counter);
        Ok(())
    }

    #[instrument(skip_all)]
    pub fn undo(&mut self, doc: &LoroDoc) -> LoroResult<()> {
        self.perform(doc, |x| &mut x.undo_stack, |x| &mut x.redo_stack)
    }

    #[instrument(skip_all)]
    pub fn redo(&mut self, doc: &LoroDoc) -> LoroResult<()> {
        self.perform(doc, |x| &mut x.redo_stack, |x| &mut x.undo_stack)
    }

    fn perform(
        &mut self,
        doc: &LoroDoc,
        get_stack: impl Fn(&mut UndoManagerInner) -> &mut Stack,
        get_opposite: impl Fn(&mut UndoManagerInner) -> &mut Stack,
    ) -> LoroResult<()> {
        self.record_new_checkpoint(doc)?;
        let end_counter = get_counter_end(doc, self.peer);
        let mut top = {
            let mut inner = self.inner.try_lock().unwrap();
            inner.processing_undo = true;
            get_stack(&mut inner).pop()
        };

        while let Some((span, e)) = top {
            trace_span!("Undo", "{:?}@{}", span, self.peer).in_scope(|| {
                {
                    let inner = self.inner.clone();
                    // TODO: Perf we can try to avoid this clone
                    let e = e.try_lock().unwrap().clone();
                    doc.undo_internal(
                        IdSpan {
                            peer: self.peer,
                            counter: span,
                        },
                        &mut self.container_remap,
                        Some(&e),
                        &mut |diff| {
                            info_span!("transform remote diff").in_scope(|| {
                                let mut inner = inner.try_lock().unwrap();
                                get_stack(&mut inner).transform_based_on_this_delta(diff);
                            });
                        },
                    )?;
                }
                Ok::<(), LoroError>(())
            })?;
            let new_counter = get_counter_end(doc, self.peer);
            if end_counter != new_counter {
                let mut inner = self.inner.try_lock().unwrap();
                get_opposite(&mut inner).push(CounterSpan::new(end_counter, new_counter));
                inner.latest_counter = new_counter;
                break;
            } else {
                // continue to pop the undo item as this undo is a no-op
                top = get_stack(&mut self.inner.try_lock().unwrap()).pop();
                continue;
            }
        }

        self.inner.try_lock().unwrap().processing_undo = false;
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.inner.try_lock().unwrap().undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.inner.try_lock().unwrap().redo_stack.is_empty()
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
    on_last_event_a: &mut dyn FnMut(&DiffBatch),
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
            if i == spans.len() - 1 {
                on_last_event_a(&event_a_prime);
            }
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
