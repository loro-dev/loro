use std::{collections::VecDeque, sync::Arc};

use crate::sync::{AtomicU64, Mutex};
use either::Either;
use fxhash::{FxHashMap, FxHashSet};
use loro_common::{
    ContainerID, Counter, CounterSpan, HasIdSpan, IdSpan, LoroError, LoroResult, LoroValue, PeerID,
};
use tracing::{debug_span, info_span, instrument};

use crate::{
    change::{get_sys_timestamp, Timestamp},
    cursor::{AbsolutePosition, Cursor},
    delta::TreeExternalDiff,
    event::{Diff, EventTriggerKind},
    version::Frontiers,
    ContainerDiff, DiffEvent, DocDiff, LoroDoc, Subscription,
};

/// A batch of diffs.
///
/// You can use `loroDoc.apply_diff(diff)` to apply the diff to the document.
#[derive(Debug, Clone, Default)]
pub struct DiffBatch {
    pub cid_to_events: FxHashMap<ContainerID, Diff>,
    pub order: Vec<ContainerID>,
}

impl DiffBatch {
    pub fn new(diff: Vec<DocDiff>) -> Self {
        let mut map: FxHashMap<ContainerID, Diff> = Default::default();
        let mut order: Vec<ContainerID> = Vec::with_capacity(diff.len());
        for d in diff.into_iter() {
            for item in d.diff.into_iter() {
                let old = map.insert(item.id.clone(), item.diff);
                assert!(old.is_none());
                order.push(item.id.clone());
            }
        }

        Self {
            cid_to_events: map,
            order,
        }
    }

    pub fn compose(&mut self, other: &Self) {
        if other.cid_to_events.is_empty() {
            return;
        }

        for (id, diff) in other.iter() {
            if let Some(this_diff) = self.cid_to_events.get_mut(id) {
                // Special handling for text diffs to avoid empty results
                if let (Diff::Text(_), Diff::Text(_)) = (this_diff.clone(), diff) {
                    // Clone to check if composition would result in empty diff
                    let mut test_diff = this_diff.clone();
                    test_diff.compose_ref(diff);
                    if test_diff.is_empty() {
                        // If composition would be empty, replace with the new diff
                        // This handles the case where insert + delete cancel out
                        *this_diff = diff.clone();
                    } else {
                        // Normal composition
                        this_diff.compose_ref(diff);
                    }
                } else {
                    this_diff.compose_ref(diff);
                }
            } else {
                self.cid_to_events.insert(id.clone(), diff.clone());
                self.order.push(id.clone());
            }
        }
    }

    pub fn transform(&mut self, other: &Self, left_priority: bool) {
        if other.cid_to_events.is_empty() || self.cid_to_events.is_empty() {
            return;
        }

        for (idx, diff) in self.cid_to_events.iter_mut() {
            if let Some(b_diff) = other.cid_to_events.get(idx) {
                diff.transform(b_diff, left_priority);
            }
        }
    }

    pub fn clear(&mut self) {
        self.cid_to_events.clear();
        self.order.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ContainerID, &Diff)> + '_ {
        self.order
            .iter()
            .map(|cid| (cid, self.cid_to_events.get(cid).unwrap()))
    }

    #[allow(clippy::should_implement_trait)]
    pub fn into_iter(self) -> impl Iterator<Item = (ContainerID, Diff)> {
        let mut cid_to_events = self.cid_to_events;
        self.order.into_iter().map(move |cid| {
            let d = cid_to_events.remove(&cid).unwrap();
            (cid, d)
        })
    }
}

fn transform_cursor(
    cursor_with_pos: &mut CursorWithPos,
    remote_diff: &DiffBatch,
    doc: &LoroDoc,
    container_remap: &FxHashMap<ContainerID, ContainerID>,
) {
    let mut cid = &cursor_with_pos.cursor.container;
    while let Some(new_cid) = container_remap.get(cid) {
        cid = new_cid;
    }

    if let Some(diff) = remote_diff.cid_to_events.get(cid) {
        let new_pos = diff.transform_cursor(cursor_with_pos.pos.pos, false);
        cursor_with_pos.pos.pos = new_pos;
    };

    let new_pos = cursor_with_pos.pos.pos;
    match doc.get_handler(cid.clone()).unwrap() {
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
pub struct UndoManager {
    peer: Arc<AtomicU64>,
    container_remap: Arc<Mutex<FxHashMap<ContainerID, ContainerID>>>,
    inner: Arc<Mutex<UndoManagerInner>>,
    _peer_id_change_sub: Subscription,
    _undo_sub: Subscription,
    doc: LoroDoc,
}

impl std::fmt::Debug for UndoManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UndoManager")
            .field("peer", &self.peer)
            .field("container_remap", &self.container_remap)
            .field("inner", &self.inner)
            .finish()
    }
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

/// When a undo/redo item is pushed, the undo manager will call the on_push callback to get the meta data of the undo item.
/// The returned cursors will be recorded for a new pushed undo item.
pub type OnPush = Box<
    dyn for<'a> Fn(UndoOrRedo, CounterSpan, Option<DiffEvent<'a>>) -> UndoItemMeta + Send + Sync,
>;
pub type OnPop = Box<dyn Fn(UndoOrRedo, CounterSpan, UndoItemMeta) + Send + Sync>;

struct UndoManagerInner {
    next_counter: Option<Counter>,
    undo_stack: Stack,
    redo_stack: Stack,
    processing_undo: bool,
    last_undo_time: i64,
    merge_interval_in_ms: i64,
    max_stack_size: usize,
    exclude_origin_prefixes: Vec<Box<str>>,
    last_popped_selection: Option<Vec<CursorWithPos>>,
    on_push: Option<OnPush>,
    on_pop: Option<OnPop>,
    group: Option<UndoGroup>,
    pending_undo_diff: DiffBatch,
}

impl std::fmt::Debug for UndoManagerInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UndoManagerInner")
            .field("latest_counter", &self.next_counter)
            .field("undo_stack", &self.undo_stack)
            .field("redo_stack", &self.redo_stack)
            .field("processing_undo", &self.processing_undo)
            .field("last_undo_time", &self.last_undo_time)
            .field("merge_interval", &self.merge_interval_in_ms)
            .field("max_stack_size", &self.max_stack_size)
            .field("exclude_origin_prefixes", &self.exclude_origin_prefixes)
            .field("group", &self.group)
            .finish()
    }
}

#[derive(Debug, Clone, Default)]
struct UndoGroup {
    start_counter: Counter,
    affected_cids: FxHashSet<ContainerID>,
}

impl UndoGroup {
    pub fn new(start_counter: Counter) -> Self {
        Self {
            start_counter,
            affected_cids: Default::default(),
        }
    }
}

#[derive(Debug)]
struct Stack {
    stack: VecDeque<(VecDeque<StackItem>, Arc<Mutex<DiffBatch>>)>,
    size: usize,
}

#[derive(Debug, Clone)]
struct StackItem {
    span: CounterSpan,
    meta: UndoItemMeta,
    undo_diff: DiffBatch,
}

/// The metadata of an undo item.
///
/// The cursors inside the metadata will be transformed by remote operations as well.
/// So that when the item is popped, users can restore the cursors position correctly.
#[derive(Debug, Default, Clone)]
pub struct UndoItemMeta {
    pub value: LoroValue,
    pub cursors: Vec<CursorWithPos>,
}

#[derive(Debug, Clone)]
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
            let diff = diff.lock().unwrap();
            if !diff.cid_to_events.is_empty() {
                self.stack
                    .back_mut()
                    .unwrap()
                    .1
                    .lock()
                    .unwrap()
                    .compose(&diff);
            }
        }

        if self.stack.len() == 1 && self.stack.back().unwrap().0.is_empty() {
            // If the stack is empty, we need to clear the remote diff
            self.stack.back_mut().unwrap().1.lock().unwrap().clear();
            return None;
        }

        self.size -= 1;
        let last = self.stack.back_mut().unwrap();
        last.0.pop_back().map(|x| (x, last.1.clone()))
        // If this row in stack is empty, we don't pop it right away
        // Because we still need the remote diff to be available.
        // Cursor position transformation relies on the remote diff in the same row.
    }

    pub fn push(&mut self, span: CounterSpan, meta: UndoItemMeta, undo_diff: DiffBatch) {
        self.push_with_merge(span, meta, undo_diff, false, None)
    }

    pub fn push_with_merge(
        &mut self,
        span: CounterSpan,
        meta: UndoItemMeta,
        undo_diff: DiffBatch,
        can_merge: bool,
        group: Option<&UndoGroup>,
    ) {
        let last = self.stack.back_mut().unwrap();
        let last_remote_diff = last.1.lock().unwrap();

        // Check if the remote diff is disjoint with the current undo group
        let is_disjoint_group = group.is_some_and(|g| {
            g.affected_cids.iter().all(|cid| {
                last_remote_diff
                    .cid_to_events
                    .get(cid)
                    .is_none_or(|diff| diff.is_empty())
            })
        });

        // Can't merge if remote diffs exist and it's not disjoint with the current undo group
        let should_create_new_entry =
            !last_remote_diff.cid_to_events.is_empty() && !is_disjoint_group;

        if should_create_new_entry {
            // Create a new entry in the stack
            drop(last_remote_diff);
            let mut v = VecDeque::new();
            v.push_back(StackItem { span, meta, undo_diff });
            self.stack
                .push_back((v, Arc::new(Mutex::new(DiffBatch::default()))));
            self.size += 1;
            return;
        }

        // Try to merge with the previous entry if allowed
        if can_merge {
            if let Some(last_span) = last.0.back_mut() {
                if last_span.span.end == span.start {
                    // Merge spans by extending the end of the last span
                    last_span.span.end = span.end;
                    // Compose the undo diffs
                    last_span.undo_diff.compose(&undo_diff);
                    return;
                }
            }
        }

        // Add as a new item to the existing entry
        self.size += 1;
        last.0.push_back(StackItem { span, meta, undo_diff });
    }

    pub fn compose_remote_event(&mut self, diff: &[&ContainerDiff]) {
        if self.is_empty() {
            return;
        }

        let remote_diff = &mut self.stack.back_mut().unwrap().1;
        let mut remote_diff = remote_diff.lock().unwrap();
        for e in diff {
            if let Some(d) = remote_diff.cid_to_events.get_mut(&e.id) {
                d.compose_ref(&e.diff);
            } else {
                remote_diff
                    .cid_to_events
                    .insert(e.id.clone(), e.diff.clone());
                remote_diff.order.push(e.id.clone());
            }
        }
    }

    pub fn transform_based_on_this_delta(&mut self, diff: &DiffBatch) {
        if self.is_empty() {
            return;
        }
        let remote_diff = &mut self.stack.back_mut().unwrap().1;
        remote_diff.lock().unwrap().transform(diff, false);
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
        Self {
            next_counter: Some(last_counter),
            undo_stack: Default::default(),
            redo_stack: Default::default(),
            processing_undo: false,
            merge_interval_in_ms: 0,
            last_undo_time: 0,
            max_stack_size: usize::MAX,
            exclude_origin_prefixes: vec![],
            last_popped_selection: None,
            on_pop: None,
            on_push: None,
            group: None,
            pending_undo_diff: Default::default(),
        }
    }

    /// Returns true if a given container diff is disjoint with the current group.
    /// They are disjoint if they have no overlap in changed container ids.
    fn is_disjoint_with_group(&self, diff: &[&ContainerDiff]) -> bool {
        let Some(group) = &self.group else {
            return false;
        };

        diff.iter().all(|d| !group.affected_cids.contains(&d.id))
    }

    fn record_checkpoint(&mut self, latest_counter: Counter, event: Option<DiffEvent>) {
        let previous_counter = self.next_counter;

        if Some(latest_counter) == self.next_counter {
            return;
        }

        if self.next_counter.is_none() {
            self.next_counter = Some(latest_counter);
            return;
        }

        if let Some(group) = &mut self.group {
            event.iter().for_each(|e| {
                e.events.iter().for_each(|e| {
                    group.affected_cids.insert(e.id.clone());
                })
            });
        }

        let now = get_sys_timestamp() as Timestamp;
        let span = CounterSpan::new(self.next_counter.unwrap(), latest_counter);
        let meta = self
            .on_push
            .as_ref()
            .map(|x| x(UndoOrRedo::Undo, span, event))
            .unwrap_or_default();

        // Wether the change is within the accepted merge interval
        let in_merge_interval = now - self.last_undo_time < self.merge_interval_in_ms;

        // If group is active, but there is nothing in the group, don't merge
        // If the group is active and it's not the first push in the group, merge
        let group_should_merge = self.group.is_some()
            && match (
                previous_counter,
                self.group.as_ref().map(|g| g.start_counter),
            ) {
                (Some(previous), Some(active)) => previous != active,
                _ => true,
            };

        let should_merge = !self.undo_stack.is_empty() && (in_merge_interval || group_should_merge);

        // Take the pending undo diff
        let undo_diff = std::mem::take(&mut self.pending_undo_diff);

        if should_merge {
            self.undo_stack
                .push_with_merge(span, meta, undo_diff, true, self.group.as_ref());
        } else {
            self.last_undo_time = now;
            self.undo_stack.push(span, meta, undo_diff);
        }

        self.next_counter = Some(latest_counter);
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
        .vv()
        .get(&peer)
        .cloned()
        .unwrap_or(0)
}

impl UndoManager {
    pub fn new(doc: &LoroDoc) -> Self {
        let peer = Arc::new(AtomicU64::new(doc.peer_id()));
        let peer_clone = peer.clone();
        let peer_clone2 = peer.clone();
        let inner = Arc::new(Mutex::new(UndoManagerInner::new(get_counter_end(
            doc,
            doc.peer_id(),
        ))));
        let inner_clone = inner.clone();
        let inner_clone2 = inner.clone();
        let remap_containers = Arc::new(Mutex::new(FxHashMap::default()));
        let remap_containers_clone = remap_containers.clone();
        let undo_sub = doc.subscribe_root(Arc::new(move |event| match event.event_meta.by {
            EventTriggerKind::Local => {
                // TODO: PERF undo can be significantly faster if we can get
                // the DiffBatch for undo here
                let Ok(mut inner) = inner_clone.lock() else {
                    return;
                };
                if inner.processing_undo {
                    return;
                }
                if let Some(id) = event
                    .event_meta
                    .to
                    .iter()
                    .find(|x| x.peer == peer_clone.load(std::sync::atomic::Ordering::Relaxed))
                {
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
                        inner.next_counter = Some(id.counter + 1);
                    } else {
                        inner.record_checkpoint(id.counter + 1, Some(event));
                    }
                }
            }
            EventTriggerKind::Import => {
                let mut inner = inner_clone.lock().unwrap();

                for e in event.events {
                    if let Diff::Tree(tree) = &e.diff {
                        for item in &tree.diff {
                            let target = item.target;
                            if let TreeExternalDiff::Create { .. } = &item.action {
                                // If the concurrent event is a create event, it may bring the deleted tree node back,
                                // so we need to remove it from the remap of the container.
                                remap_containers_clone
                                    .lock()
                                    .unwrap()
                                    .remove(&target.associated_meta_container());
                            }
                        }
                    }
                }

                let is_import_disjoint = inner.is_disjoint_with_group(event.events);

                inner.undo_stack.compose_remote_event(event.events);
                inner.redo_stack.compose_remote_event(event.events);

                // If the import is not disjoint, we end the active group
                // all subsequent changes will be new undo items
                if !is_import_disjoint {
                    inner.group = None;
                }
            }
            EventTriggerKind::Checkout => {
                let mut inner = inner_clone.lock().unwrap();
                inner.undo_stack.clear();
                inner.redo_stack.clear();
                inner.next_counter = None;
            }
        }));

        let sub = doc.subscribe_peer_id_change(Box::new(move |id| {
            let mut inner = inner_clone2.lock().unwrap();
            inner.undo_stack.clear();
            inner.redo_stack.clear();
            inner.next_counter = Some(id.counter);
            peer_clone2.store(id.peer, std::sync::atomic::Ordering::Relaxed);
            true
        }));

        UndoManager {
            peer,
            container_remap: remap_containers,
            inner,
            _peer_id_change_sub: sub,
            _undo_sub: undo_sub,
            doc: doc.clone(),
        }
    }

    pub fn group_start(&mut self) -> LoroResult<()> {
        let mut inner = self.inner.lock().unwrap();

        if inner.group.is_some() {
            return Err(LoroError::UndoGroupAlreadyStarted);
        }

        inner.group = Some(UndoGroup::new(inner.next_counter.unwrap()));

        Ok(())
    }

    pub fn group_end(&mut self) {
        self.inner.lock().unwrap().group = None;
    }

    pub fn peer(&self) -> PeerID {
        self.peer.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_merge_interval(&mut self, interval: i64) {
        self.inner.lock().unwrap().merge_interval_in_ms = interval;
    }

    pub fn set_max_undo_steps(&mut self, size: usize) {
        self.inner.lock().unwrap().max_stack_size = size;
    }

    pub fn add_exclude_origin_prefix(&mut self, prefix: &str) {
        self.inner
            .lock()
            .unwrap()
            .exclude_origin_prefixes
            .push(prefix.into());
    }

    pub fn record_new_checkpoint(&mut self) -> LoroResult<()> {
        self.doc.commit_then_renew();
        let counter = get_counter_end(&self.doc, self.peer());
        self.inner.lock().unwrap().record_checkpoint(counter, None);
        Ok(())
    }

    #[instrument(skip_all)]
    pub fn undo(&mut self) -> LoroResult<bool> {
        self.perform(
            |x| &mut x.undo_stack,
            |x| &mut x.redo_stack,
            UndoOrRedo::Undo,
        )
    }

    #[instrument(skip_all)]
    pub fn redo(&mut self) -> LoroResult<bool> {
        self.perform(
            |x| &mut x.redo_stack,
            |x| &mut x.undo_stack,
            UndoOrRedo::Redo,
        )
    }

    fn perform(
        &mut self,
        get_stack: impl Fn(&mut UndoManagerInner) -> &mut Stack,
        get_opposite: impl Fn(&mut UndoManagerInner) -> &mut Stack,
        kind: UndoOrRedo,
    ) -> LoroResult<bool> {
        let doc = &self.doc.clone();
        // When in the undo/redo loop, the new undo/redo stack item should restore the selection
        // to the state it was in before the item that was popped two steps ago from the stack.
        //
        //                          ┌────────────┐
        //                          │Selection 1 │
        //                          └─────┬──────┘
        //                                │   Some
        //                                ▼   ops
        //                          ┌────────────┐
        //                          │Selection 2 │
        //                          └─────┬──────┘
        //                                │   Some
        //                                ▼   ops
        //                          ┌────────────┐
        //                          │Selection 3 │◁ ─ ─ ─ ─ ─ ─ ─  Restore  ─ ─ ─
        //                          └─────┬──────┘                               │
        //                                │
        //                                │                                      │
        //                                │                              ┌ ─ ─ ─ ─ ─ ─ ─
        //           Enter the            │   Undo ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─▶   Push Redo   │
        //           undo/redo ─ ─ ─ ▶    ▼                              └ ─ ─ ─ ─ ─ ─ ─
        //             loop         ┌────────────┐                               │
        //                          │Selection 2 │◁─ ─ ─  Restore  ─
        //                          └─────┬──────┘                  │            │
        //                                │
        //                                │                         │            │
        //                                │                 ┌ ─ ─ ─ ─ ─ ─ ─
        //                                │   Undo ─ ─ ─ ─ ▶   Push Redo   │     │
        //                                ▼                 └ ─ ─ ─ ─ ─ ─ ─
        //                          ┌────────────┐                  │            │
        //                          │Selection 1 │
        //                          └─────┬──────┘                  │            │
        //                                │   Redo ◀ ─ ─ ─ ─ ─ ─ ─ ─
        //                                ▼                                      │
        //                          ┌────────────┐
        //         ┌   Restore   ─ ▷│Selection 2 │                               │
        //                          └─────┬──────┘
        //         │                      │                                      │
        // ┌ ─ ─ ─ ─ ─ ─ ─                │
        //    Push Undo   │◀─ ─ ─ ─ ─ ─ ─ │   Redo ◀ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘
        // └ ─ ─ ─ ─ ─ ─ ─                ▼
        //         │                ┌────────────┐
        //                          │Selection 3 │
        //         │                └─────┬──────┘
        //          ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ▶ │   Undo
        //                                ▼
        //                          ┌────────────┐
        //                          │Selection 2 │
        //                          └────────────┘
        //
        // Because users may change the selections during the undo/redo loop, it's
        // more stable to keep the selection stored in the last stack item
        // rather than using the current selection directly.
        self.record_new_checkpoint()?;
        let end_counter = get_counter_end(doc, self.peer());
        let mut top = {
            let mut inner = self.inner.lock().unwrap();
            inner.processing_undo = true;
            get_stack(&mut inner).pop()
        };

        let mut executed = false;
        while let Some((mut span, remote_diff)) = top {
            let mut next_push_selection = None;
            
            // Check if we have a precalculated undo diff
            let use_precalculated_diff = !span.undo_diff.cid_to_events.is_empty();
            
            if use_precalculated_diff {
                // Optimized path: use precalculated diff (avoids checkouts!)
                debug_span!("Using precalculated undo diff - no checkouts").in_scope(|| {
                    // Transform the undo diff based on remote changes
                    let mut undo_diff = span.undo_diff.clone();
                    let remote_change_clone = remote_diff.lock().unwrap().clone();
                    undo_diff.transform(&remote_change_clone, true);
                    
                    // Clear pending_undo_diff before applying to capture redo diff
                    {
                        let mut inner = self.inner.lock().unwrap();
                        inner.pending_undo_diff.clear();
                    }
                    
                    // Apply the transformed undo diff
                    doc._apply_diff(
                        undo_diff,
                        &mut self.container_remap.lock().unwrap(),
                        true
                    ).unwrap();
                    
                    // Transform the stack based on the generated diff
                    let inner = self.inner.clone();
                    let pending_diff = inner.lock().unwrap().pending_undo_diff.clone();
                    if !pending_diff.cid_to_events.is_empty() {
                        info_span!("transform remote diff").in_scope(|| {
                            let mut inner = inner.lock().unwrap();
                            get_stack(&mut inner).transform_based_on_this_delta(&pending_diff);
                        });
                    }
                });
                
                let mut inner = self.inner.lock().unwrap();
                if let Some(x) = inner.on_pop.as_ref() {
                    for cursor in span.meta.cursors.iter_mut() {
                        transform_cursor(
                            cursor,
                            &remote_diff.lock().unwrap(),
                            doc,
                            &self.container_remap.lock().unwrap(),
                        );
                    }

                    x(kind, span.span, span.meta.clone());
                    let take = inner.last_popped_selection.take();
                    next_push_selection = take;
                    inner.last_popped_selection = Some(span.meta.cursors);
                }
            } else {
                // Fallback path: use undo_internal for backward compatibility
                // This path is needed for:
                // 1. Operations created before UndoManager initialization
                // 2. Operations imported from other peers
                // 3. Legacy documents without precalculated diffs
                // NOTE: This path performs checkouts and is slower (O(n²) complexity)
                let inner = self.inner.clone();
                // We need to clone this because otherwise <transform_delta> will be applied to the same remote diff
                let remote_change_clone = remote_diff.lock().unwrap().clone();
                
                // Clear pending_undo_diff before undo_internal to capture redo diff
                {
                    let mut inner = self.inner.lock().unwrap();
                    inner.pending_undo_diff.clear();
                }
                
                let commit = doc.undo_internal(
                    IdSpan {
                        peer: self.peer(),
                        counter: span.span,
                    },
                    &mut self.container_remap.lock().unwrap(),
                    Some(&remote_change_clone),
                    &mut |diff| {
                        info_span!("transform remote diff").in_scope(|| {
                            let mut inner = inner.lock().unwrap();
                            // <transform_delta>
                            get_stack(&mut inner).transform_based_on_this_delta(diff);
                        });
                    },
                )?;
                drop(commit);
                let mut inner = self.inner.lock().unwrap();
                if let Some(x) = inner.on_pop.as_ref() {
                    for cursor in span.meta.cursors.iter_mut() {
                        // <cursor_transform> We need to transform cursor here.
                        // Note that right now <transform_delta> is already done,
                        // remote_diff is also transformed by it now (that's what we need).
                        transform_cursor(
                            cursor,
                            &remote_diff.lock().unwrap(),
                            doc,
                            &self.container_remap.lock().unwrap(),
                        );
                    }

                    x(kind, span.span, span.meta.clone());
                    let take = inner.last_popped_selection.take();
                    next_push_selection = take;
                    inner.last_popped_selection = Some(span.meta.cursors);
                }
            }
            let new_counter = get_counter_end(doc, self.peer());
            if end_counter != new_counter {
                let mut inner = self.inner.lock().unwrap();
                let mut meta = inner
                    .on_push
                    .as_ref()
                    .map(|x| {
                        x(
                            kind.opposite(),
                            CounterSpan::new(end_counter, new_counter),
                            None,
                        )
                    })
                    .unwrap_or_default();

                if matches!(kind, UndoOrRedo::Undo) && get_opposite(&mut inner).is_empty() {
                    // If it's the first undo, we use the cursors from the users
                } else if let Some(inner) = next_push_selection.take() {
                    // Otherwise, we use the cursors from the undo/redo loop
                    meta.cursors = inner;
                }

                // Take the redo diff that was collected during the undo operation
                let redo_diff = if !inner.pending_undo_diff.cid_to_events.is_empty() {
                    std::mem::take(&mut inner.pending_undo_diff)
                } else {
                    // If no redo diff was collected (using old path), create an empty one
                    // The old path will still work but won't benefit from the optimization
                    Default::default()
                };
                
                get_opposite(&mut inner).push(CounterSpan::new(end_counter, new_counter), meta, redo_diff);
                inner.next_counter = Some(new_counter);
                executed = true;
                break;
            } else {
                // continue to pop the undo item as this undo is a no-op
                top = get_stack(&mut self.inner.lock().unwrap()).pop();
                continue;
            }
        }

        self.inner.lock().unwrap().processing_undo = false;
        Ok(executed)
    }

    pub fn can_undo(&self) -> bool {
        !self.inner.lock().unwrap().undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.inner.lock().unwrap().redo_stack.is_empty()
    }

    pub fn undo_count(&self) -> usize {
        self.inner.lock().unwrap().undo_stack.len()
    }

    pub fn redo_count(&self) -> usize {
        self.inner.lock().unwrap().redo_stack.len()
    }

    pub fn set_on_push(&self, on_push: Option<OnPush>) {
        self.inner.lock().unwrap().on_push = on_push;
    }

    pub fn set_on_pop(&self, on_pop: Option<OnPop>) {
        self.inner.lock().unwrap().on_pop = on_pop;
    }

    pub fn clear(&self) {
        self.inner.lock().unwrap().undo_stack.clear();
        self.inner.lock().unwrap().redo_stack.clear();
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

            // println!("event_a_i: {:?}", event_a_i);

            // ---------------------------------------
            // 2. Calc event B_i
            // ---------------------------------------
            let stack_diff_batch;
            let event_b_i = 'block: {
                let next = if i + 1 < spans.len() {
                    spans[i + 1].0.id_last().into()
                } else {
                    match last_frontiers_or_last_bi {
                        Either::Left(last_frontiers) => last_frontiers.clone(),
                        Either::Right(right) => break 'block right,
                    }
                };
                stack_diff_batch = Some(calc_diff(&this_id_span.id_last().into(), &next));
                stack_diff_batch.as_ref().unwrap()
            };

            // println!("event_b_i: {:?}", event_b_i);

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

            // println!("event_a_prime: {:?}", event_a_prime);

            let c_i = event_a_prime;
            last_ci = Some(c_i);
        });
    }

    last_ci.unwrap()
}
