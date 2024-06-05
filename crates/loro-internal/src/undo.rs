use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use either::Either;
use fxhash::FxHashMap;
use loro_common::{
    ContainerID, Counter, CounterSpan, HasCounterSpan, HasIdSpan, IdSpan, LoroError, LoroResult,
    LoroValue, PeerID,
};
use tracing::{debug_span, info_span, instrument, trace};

use crate::{
    change::get_sys_timestamp,
    cursor::{AbsolutePosition, Cursor},
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
        if other.0.is_empty() || self.0.is_empty() {
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

fn transform_cursor(
    cursor_with_pos: &mut CursorWithPos,
    remote_diff: &DiffBatch,
    doc: &LoroDoc,
    container_remap: &FxHashMap<ContainerID, ContainerID>,
) {
    let mut cid = &cursor_with_pos.cursor.container;
    while let Some(new_cid) = container_remap.get(&cid) {
        cid = new_cid;
    }

    if let Some(diff) = remote_diff.0.get(cid) {
        let new_pos = diff.transform_cursor(cursor_with_pos.pos.pos, false);
        cursor_with_pos.pos.pos = new_pos;
    };

    let new_pos = cursor_with_pos.pos.pos;
    match doc.get_handler(cid.clone()) {
        crate::handler::Handler::Text(h) => {
            let Some(new_cursor) = h.get_cursor_internal(new_pos, cursor_with_pos.pos.side, false)
            else {
                return;
            };

            cursor_with_pos.cursor = new_cursor;
        }
        crate::handler::Handler::List(h) => {
            let Some(new_cursor) = h.get_cursor(new_pos, cursor_with_pos.pos.side) else {
                return;
            };

            cursor_with_pos.cursor = new_cursor;
        }
        crate::handler::Handler::MovableList(h) => {
            let Some(new_cursor) = h.get_cursor(new_pos, cursor_with_pos.pos.side) else {
                return;
            };

            cursor_with_pos.cursor = new_cursor;
        }
        crate::handler::Handler::Map(_) => {}
        crate::handler::Handler::Tree(_) => {}
        crate::handler::Handler::Unknown(_) => {}
        #[cfg(feature = "counter")]
        crate::handler::Handler::Counter(_) => {}
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoOrRedo {
    Undo,
    Redo,
}

impl UndoOrRedo {
    fn opposite(&self) -> UndoOrRedo {
        match self {
            Self::Undo => Self::Redo,
            Self::Redo => Self::Undo,
        }
    }
}

pub type OnPush = Box<dyn Fn(UndoOrRedo, CounterSpan) -> UndoItemMeta + Send + Sync>;
pub type OnPop = Box<dyn Fn(UndoOrRedo, CounterSpan, UndoItemMeta) + Send + Sync>;

struct UndoManagerInner {
    latest_counter: Option<Counter>,
    undo_stack: Stack,
    redo_stack: Stack,
    processing_undo: bool,
    last_undo_time: i64,
    merge_interval: i64,
    max_stack_size: usize,
    exclude_origin_prefixes: Vec<Box<str>>,
    on_push: Option<OnPush>,
    on_pop: Option<OnPop>,
}

impl std::fmt::Debug for UndoManagerInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UndoManagerInner")
            .field("latest_counter", &self.latest_counter)
            .field("undo_stack", &self.undo_stack)
            .field("redo_stack", &self.redo_stack)
            .field("processing_undo", &self.processing_undo)
            .field("last_undo_time", &self.last_undo_time)
            .field("merge_interval", &self.merge_interval)
            .field("max_stack_size", &self.max_stack_size)
            .field("exclude_origin_prefixes", &self.exclude_origin_prefixes)
            .finish()
    }
}

#[derive(Debug)]
struct Stack {
    stack: VecDeque<(VecDeque<StackItem>, Arc<Mutex<DiffBatch>>)>,
    size: usize,
}

#[derive(Debug)]
struct StackItem {
    span: CounterSpan,
    meta: UndoItemMeta,
}

/// The metadata of an undo item.
///
/// The cursors inside the metadata will be transformed by remote operations as well.
/// So that when the item is popped, users can restore the cursors position correctly.
#[derive(Debug, Default)]
pub struct UndoItemMeta {
    pub value: LoroValue,
    pub cursors: Vec<CursorWithPos>,
}

#[derive(Debug)]
pub struct CursorWithPos {
    pub cursor: Cursor,
    pub pos: AbsolutePosition,
}

impl UndoItemMeta {
    pub fn new() -> Self {
        Self {
            value: LoroValue::Null,
            cursors: Default::default(),
        }
    }

    /// It's assumed that the cursor is just acquired before the ops that
    /// need to be undo/redo.
    ///
    /// We need to rely on the validity of the original pos value
    pub fn add_cursor(&mut self, cursor: &Cursor) {
        self.cursors.push(CursorWithPos {
            cursor: cursor.clone(),
            pos: AbsolutePosition {
                pos: cursor.origin_pos,
                side: cursor.side,
            },
        });
    }

    pub fn set_value(&mut self, value: LoroValue) {
        self.value = value;
    }
}

impl Stack {
    pub fn new() -> Self {
        let mut stack = VecDeque::new();
        stack.push_back((VecDeque::new(), Arc::new(Mutex::new(Default::default()))));
        Stack { stack, size: 0 }
    }

    pub fn pop(&mut self) -> Option<(StackItem, Arc<Mutex<DiffBatch>>)> {
        while self.stack.back().unwrap().0.is_empty() && self.stack.len() > 1 {
            let (_, diff) = self.stack.pop_back().unwrap();
            let diff = diff.try_lock().unwrap();
            if !diff.0.is_empty() {
                self.stack
                    .back_mut()
                    .unwrap()
                    .1
                    .try_lock()
                    .unwrap()
                    .compose(&diff);
            }
        }

        if self.stack.len() == 1 && self.stack.back().unwrap().0.is_empty() {
            // If the stack is empty, we need to clear the remote diff
            self.stack.back_mut().unwrap().1.try_lock().unwrap().clear();
            return None;
        }

        self.size -= 1;
        let last = self.stack.back_mut().unwrap();
        last.0.pop_back().map(|x| (x, last.1.clone()))
        // If this row in stack is empty, we don't pop it right away
        // Because we still need the remote diff to be available.
        // Cursor position transformation relies on the remote diff in the same row.
    }

    pub fn push(&mut self, span: CounterSpan, meta: UndoItemMeta) {
        self.push_with_merge(span, meta, false)
    }

    pub fn push_with_merge(&mut self, span: CounterSpan, meta: UndoItemMeta, can_merge: bool) {
        let last = self.stack.back_mut().unwrap();
        let mut last_remote_diff = last.1.try_lock().unwrap();
        if !last_remote_diff.0.is_empty() {
            // If the remote diff is not empty, we cannot merge
            if last.0.is_empty() {
                last.0.push_back(StackItem { span, meta });
                last_remote_diff.clear();
            } else {
                drop(last_remote_diff);
                let mut v = VecDeque::new();
                v.push_back(StackItem { span, meta });
                self.stack
                    .push_back((v, Arc::new(Mutex::new(DiffBatch::default()))));
            }

            self.size += 1;
        } else {
            if can_merge {
                if let Some(last_span) = last.0.back_mut() {
                    if last_span.span.end == span.start {
                        // merge the span
                        last_span.span.end = span.end;
                        return;
                    }
                }
            }

            self.size += 1;
            last.0.push_back(StackItem { span, meta });
        }
    }

    pub fn compose_remote_event(&mut self, diff: &[&ContainerDiff]) {
        if self.is_empty() {
            return;
        }

        let remote_diff = &mut self.stack.back_mut().unwrap().1;
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

        let remote_diff = &mut self.stack.back_mut().unwrap().1;
        remote_diff.try_lock().unwrap().transform(diff, false);
    }

    pub fn clear(&mut self) {
        self.stack = VecDeque::new();
        self.stack.push_back((VecDeque::new(), Default::default()));
        self.size = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    pub fn len(&self) -> usize {
        self.size
    }

    fn pop_front(&mut self) {
        if self.is_empty() {
            return;
        }

        self.size -= 1;
        let first = self.stack.front_mut().unwrap();
        let f = first.0.pop_front();
        assert!(f.is_some());
        if first.0.is_empty() {
            self.stack.pop_front();
        }
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
            latest_counter: Some(last_counter),
            undo_stack: Default::default(),
            redo_stack: Default::default(),
            processing_undo: false,
            merge_interval: 0,
            last_undo_time: 0,
            max_stack_size: usize::MAX,
            exclude_origin_prefixes: vec![],
            on_pop: None,
            on_push: None,
        }
    }

    fn record_checkpoint(&mut self, latest_counter: Counter) {
        if Some(latest_counter) == self.latest_counter {
            return;
        }

        if self.latest_counter.is_none() {
            self.latest_counter = Some(latest_counter);
            return;
        }

        assert!(self.latest_counter.unwrap() < latest_counter);
        let now = get_sys_timestamp();
        let span = CounterSpan::new(self.latest_counter.unwrap(), latest_counter);
        let meta = self
            .on_push
            .as_ref()
            .map(|x| x(UndoOrRedo::Undo, span))
            .unwrap_or_default();
        if !self.undo_stack.is_empty() && now - self.last_undo_time < self.merge_interval {
            self.undo_stack.push_with_merge(span, meta, true);
        } else {
            self.last_undo_time = now;
            self.undo_stack.push(span, meta);
        }

        self.latest_counter = Some(latest_counter);
        self.redo_stack.clear();
        while self.undo_stack.len() > self.max_stack_size {
            self.undo_stack.pop_front();
        }
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
                // TODO: PERF undo can be significantly faster if we can get
                // the DiffBatch for undo here
                let Ok(mut inner) = inner_clone.try_lock() else {
                    return;
                };
                if inner.processing_undo {
                    return;
                }
                if let Some(id) = event.event_meta.to.iter().find(|x| x.peer == peer) {
                    if inner
                        .exclude_origin_prefixes
                        .iter()
                        .any(|x| event.event_meta.origin.starts_with(&**x))
                    {
                        // If the event is from the excluded origin, we don't record it
                        // in the undo stack. But we need to record its effect like it's
                        // a remote event.
                        inner.undo_stack.compose_remote_event(event.events);
                        inner.redo_stack.compose_remote_event(event.events);
                        inner.latest_counter = Some(id.counter + 1);
                    } else {
                        inner.record_checkpoint(id.counter + 1);
                    }
                }
            }
            EventTriggerKind::Import => {
                let mut inner = inner_clone.try_lock().unwrap();
                inner.undo_stack.compose_remote_event(event.events);
                inner.redo_stack.compose_remote_event(event.events);
            }
            EventTriggerKind::Checkout => {
                let mut inner = inner_clone.try_lock().unwrap();
                inner.undo_stack.clear();
                inner.redo_stack.clear();
                inner.latest_counter = None;
            }
        }));

        UndoManager {
            peer,
            container_remap: Default::default(),
            inner,
        }
    }

    pub fn peer(&self) -> PeerID {
        self.peer
    }

    pub fn set_merge_interval(&mut self, interval: i64) {
        self.inner.try_lock().unwrap().merge_interval = interval;
    }

    pub fn set_max_undo_steps(&mut self, size: usize) {
        self.inner.try_lock().unwrap().max_stack_size = size;
    }

    pub fn add_exclude_origin_prefix(&mut self, prefix: &str) {
        self.inner
            .try_lock()
            .unwrap()
            .exclude_origin_prefixes
            .push(prefix.into());
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
    pub fn undo(&mut self, doc: &LoroDoc) -> LoroResult<bool> {
        self.perform(
            doc,
            |x| &mut x.undo_stack,
            |x| &mut x.redo_stack,
            UndoOrRedo::Undo,
        )
    }

    #[instrument(skip_all)]
    pub fn redo(&mut self, doc: &LoroDoc) -> LoroResult<bool> {
        self.perform(
            doc,
            |x| &mut x.redo_stack,
            |x| &mut x.undo_stack,
            UndoOrRedo::Redo,
        )
    }

    fn perform(
        &mut self,
        doc: &LoroDoc,
        get_stack: impl Fn(&mut UndoManagerInner) -> &mut Stack,
        get_opposite: impl Fn(&mut UndoManagerInner) -> &mut Stack,
        kind: UndoOrRedo,
    ) -> LoroResult<bool> {
        self.record_new_checkpoint(doc)?;
        let end_counter = get_counter_end(doc, self.peer);
        let mut top = {
            let mut inner = self.inner.try_lock().unwrap();
            inner.processing_undo = true;
            get_stack(&mut inner).pop()
        };

        let mut executed = false;
        while let Some((mut span, remote_diff)) = top {
            {
                let inner = self.inner.clone();
                // We need to clone this because otherwise <transform_delta> will be applied to the same remote diff
                let remote_change_clone = remote_diff.try_lock().unwrap().clone();
                trace!("NEW Remote change {:?}", &remote_change_clone);
                let commit = doc.undo_internal(
                    IdSpan {
                        peer: self.peer,
                        counter: span.span,
                    },
                    &mut self.container_remap,
                    Some(&remote_change_clone),
                    &mut |diff| {
                        info_span!("transform remote diff").in_scope(|| {
                            let mut inner = inner.try_lock().unwrap();
                            // <transform_delta>
                            get_stack(&mut inner).transform_based_on_this_delta(diff);
                        });
                    },
                )?;
                drop(commit);
                if let Some(x) = self.inner.try_lock().unwrap().on_pop.as_ref() {
                    for cursor in span.meta.cursors.iter_mut() {
                        // <cursor_transform> We need to transform cursor here.
                        // Note that right now <transform_delta> is already done,
                        // remote_diff is also transformed by it now (that's what we need).
                        transform_cursor(
                            cursor,
                            &remote_diff.try_lock().unwrap(),
                            doc,
                            &self.container_remap,
                        );
                    }
                    x(kind, span.span, span.meta)
                }
            }
            let new_counter = get_counter_end(doc, self.peer);
            if end_counter != new_counter {
                let mut inner = self.inner.try_lock().unwrap();
                let meta = inner
                    .on_push
                    .as_ref()
                    .map(|x| x(kind.opposite(), CounterSpan::new(end_counter, new_counter)))
                    .unwrap_or_default();
                get_opposite(&mut inner).push(CounterSpan::new(end_counter, new_counter), meta);
                inner.latest_counter = Some(new_counter);
                executed = true;
                break;
            } else {
                // continue to pop the undo item as this undo is a no-op
                top = get_stack(&mut self.inner.try_lock().unwrap()).pop();
                continue;
            }
        }

        self.inner.try_lock().unwrap().processing_undo = false;
        Ok(executed)
    }

    pub fn can_undo(&self) -> bool {
        !self.inner.try_lock().unwrap().undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.inner.try_lock().unwrap().redo_stack.is_empty()
    }

    pub fn set_on_push(&self, on_push: Option<OnPush>) {
        self.inner.try_lock().unwrap().on_push = on_push;
    }

    pub fn set_on_pop(&self, on_pop: Option<OnPop>) {
        self.inner.try_lock().unwrap().on_pop = on_pop;
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

            // ---------------------------------------
            // 2. Calc event B_i
            // ---------------------------------------
            let stack_diff_batch;
            let event_b_i = 'block: {
                let next = if i + 1 < spans.len() {
                    spans[i + 1].0.id_last().into()
                } else {
                    match last_frontiers_or_last_bi.clone() {
                        Either::Left(last_frontiers) => last_frontiers.clone(),
                        Either::Right(right) => break 'block right,
                    }
                };
                stack_diff_batch = Some(calc_diff(&this_id_span.id_last().into(), &next));
                stack_diff_batch.as_ref().unwrap()
            };

            // event_a_prime can undo the ops in the current span and the previous spans
            let mut event_a_prime = if let Some(mut last_ci) = last_ci.take() {
                // ------------------------------------------------------------------------------
                // 1.b Transform and apply Ci-1 based on Ai, call it A'i
                // ------------------------------------------------------------------------------

                last_ci.transform(&event_a_i, true);

                event_a_i.compose(&last_ci);
                event_a_i
            } else {
                event_a_i
            };
            if i == spans.len() - 1 {
                on_last_event_a(&event_a_prime);
            }

            // --------------------------------------------------
            // 3. Transform event A'_i based on B_i, call it C_i
            // --------------------------------------------------
            event_a_prime.transform(event_b_i, true);
            let c_i = event_a_prime;

            last_ci = Some(c_i);
        });
    }

    last_ci.unwrap()
}
