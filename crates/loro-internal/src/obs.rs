use std::sync::{
    atomic::{AtomicU32, AtomicUsize, Ordering},
    Arc, Mutex,
};

use fxhash::{FxHashMap, FxHashSet};
use itertools::Itertools;
use loro_common::ContainerID;
use smallvec::SmallVec;

use crate::{container::idx::ContainerIdx, ContainerDiff};

use super::{
    arena::SharedArena,
    event::{DiffEvent, DocDiff},
};

pub type Subscriber = Arc<dyn (for<'a> Fn(DiffEvent<'a>)) + Send + Sync>;

#[derive(Default)]
struct ObserverInner {
    subscribers: FxHashMap<SubID, Subscriber>,
    containers: FxHashMap<ContainerIdx, FxHashSet<SubID>>,
    root: FxHashSet<SubID>,
    deleted: FxHashSet<SubID>,
    event_queue: Vec<DocDiff>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SubID(u32);

impl SubID {
    pub fn into_u32(self) -> u32 {
        self.0
    }

    pub fn from_u32(id: u32) -> Self {
        Self(id)
    }
}

pub struct Observer {
    inner: Mutex<ObserverInner>,
    arena: SharedArena,
    next_sub_id: AtomicU32,
    taken_times: AtomicUsize,
}

impl std::fmt::Debug for Observer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Observer")
            .field("next_sub_id", &self.next_sub_id)
            .field("taken_times", &self.taken_times)
            .finish()
    }
}

impl Observer {
    pub fn new(arena: SharedArena) -> Self {
        Self {
            arena,
            next_sub_id: AtomicU32::new(0),
            taken_times: AtomicUsize::new(0),
            inner: Mutex::new(ObserverInner {
                subscribers: Default::default(),
                containers: Default::default(),
                root: Default::default(),
                deleted: Default::default(),
                event_queue: Default::default(),
            }),
        }
    }

    pub fn subscribe(&self, id: &ContainerID, callback: Subscriber) -> SubID {
        let idx = self.arena.register_container(id);
        let sub_id = self.fetch_add_next_id();
        let mut inner = self.inner.lock().unwrap();
        inner.subscribers.insert(sub_id, callback);
        inner.containers.entry(idx).or_default().insert(sub_id);
        sub_id
    }

    pub fn subscribe_root(&self, callback: Subscriber) -> SubID {
        let sub_id = self.fetch_add_next_id();
        let mut inner = self.inner.lock().unwrap();
        inner.subscribers.insert(sub_id, callback);
        inner.root.insert(sub_id);
        sub_id
    }

    fn fetch_add_next_id(&self) -> SubID {
        SubID(
            self.next_sub_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        )
    }

    pub(crate) fn emit(&self, doc_diff: DocDiff) {
        if self.taken_times.load(Ordering::Relaxed) > 0 {
            self.inner.lock().unwrap().event_queue.push(doc_diff);
            return;
        }

        let mut inner = self.take_inner();
        self.emit_inner(&doc_diff, &mut inner);
        self.reset_inner(inner);
    }

    // When emitting changes, we need to make sure that the observer is not locked.
    fn emit_inner(&self, doc_diff: &DocDiff, inner: &mut ObserverInner) {
        let mut container_events_map: FxHashMap<ContainerIdx, SmallVec<[&ContainerDiff; 1]>> =
            Default::default();
        for container_diff in doc_diff.diff.iter() {
            self.arena
                .with_ancestors(container_diff.idx, |ancestor, _| {
                    if let Some(subs) = inner.containers.get_mut(&ancestor) {
                        // update subscriber set on ancestors' listener entries
                        subs.retain(|sub| match inner.subscribers.contains_key(sub) {
                            true => {
                                container_events_map
                                    .entry(ancestor)
                                    .or_default()
                                    .push(container_diff);
                                true
                            }
                            false => false,
                        });
                    }
                });
        }

        for (container_idx, container_diffs) in container_events_map {
            let subs = inner.containers.get_mut(&container_idx).unwrap();
            for sub in subs.iter() {
                let f = inner.subscribers.get_mut(sub).unwrap();
                (f)(DiffEvent {
                    current_target: Some(self.arena.get_container_id(container_idx).unwrap()),
                    events: &container_diffs,
                    event_meta: doc_diff,
                })
            }
        }

        if !inner.root.is_empty() {
            let events = doc_diff.diff.iter().collect_vec();
            inner
                .root
                // use `.retain` to update subscriber set on ancestors' listener entries
                .retain(|sub| match inner.subscribers.get_mut(sub) {
                    Some(f) => {
                        (f)(DiffEvent {
                            current_target: None,
                            events: &events,
                            event_meta: doc_diff,
                        });
                        true
                    }
                    None => false,
                })
        }
    }

    fn take_inner(&self) -> ObserverInner {
        self.taken_times
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut inner_guard = self.inner.lock().unwrap();
        std::mem::take(&mut *inner_guard)
    }

    fn reset_inner(&self, mut inner: ObserverInner) {
        let mut count = 0;
        loop {
            let mut inner_guard = self.inner.lock().unwrap();
            // need to merge the old and new sets
            if !inner_guard.containers.is_empty() {
                for (key, set) in inner_guard.containers.iter() {
                    let old_set = inner.containers.get_mut(key).unwrap();
                    for value in set {
                        old_set.insert(*value);
                    }
                }
            }

            if !inner_guard.root.is_empty() {
                for value in inner_guard.root.iter() {
                    inner.root.insert(*value);
                }
            }

            if !inner_guard.subscribers.is_empty() {
                for (key, value) in std::mem::take(&mut inner_guard.subscribers) {
                    inner.subscribers.insert(key, value);
                }
            }

            if !inner_guard.deleted.is_empty() {
                let is_taken = self.is_taken();
                for value in inner_guard.deleted.iter() {
                    inner.subscribers.remove(value);
                    if is_taken {
                        inner.deleted.insert(*value);
                    }
                }
            }

            if 1 == self
                .taken_times
                .fetch_sub(1, std::sync::atomic::Ordering::Release)
                && !inner_guard.event_queue.is_empty()
            {
                // emit the queued events
                let events = std::mem::take(&mut inner_guard.event_queue);
                *inner_guard = Default::default();
                self.taken_times
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                drop(inner_guard);
                for event in events {
                    self.emit_inner(&event, &mut inner);
                }
                count += 1;
                if count >= 1024 {
                    panic!("Too many recursive events.");
                }
            } else {
                inner.event_queue.append(&mut inner_guard.event_queue);
                *inner_guard = inner;
                return;
            }
        }
    }

    pub fn unsubscribe(&self, sub_id: SubID) {
        let mut inner = self.inner.try_lock().unwrap();
        inner.subscribers.remove(&sub_id);
        if self.is_taken() {
            inner.deleted.insert(sub_id);
        }
    }

    fn is_taken(&self) -> bool {
        self.taken_times.load(std::sync::atomic::Ordering::Acquire) != 0
    }
}

#[cfg(test)]
mod test {

    use crate::{handler::HandlerTrait, loro::LoroDoc};

    use super::*;

    #[test]
    fn test_recursive_events() {
        let loro = Arc::new(LoroDoc::new());
        let loro_cp = loro.clone();
        let count = Arc::new(AtomicUsize::new(0));
        let count_cp = Arc::clone(&count);
        loro_cp.subscribe_root(Arc::new(move |_| {
            count_cp.fetch_add(1, Ordering::SeqCst);
            let mut txn = loro.txn().unwrap();
            let text = loro.get_text("id");
            if text.get_value().as_string().unwrap().len() > 10 {
                return;
            }
            text.insert_with_txn(&mut txn, 0, "123").unwrap();
            txn.commit().unwrap();
        }));

        let loro = loro_cp;
        let mut txn = loro.txn().unwrap();
        let text = loro.get_text("id");
        text.insert_with_txn(&mut txn, 0, "123").unwrap();
        txn.commit().unwrap();
        let count = count.load(Ordering::SeqCst);
        assert!(count > 2, "{}", count);
    }

    #[test]
    fn unsubscribe() {
        let loro = Arc::new(LoroDoc::new());
        let count = Arc::new(AtomicUsize::new(0));
        let count_cp = Arc::clone(&count);
        let sub = loro.subscribe_root(Arc::new(move |_| {
            count_cp.fetch_add(1, Ordering::SeqCst);
        }));

        let text = loro.get_text("id");

        assert_eq!(count.load(Ordering::SeqCst), 0);
        {
            let mut txn = loro.txn().unwrap();
            text.insert_with_txn(&mut txn, 0, "123").unwrap();
            txn.commit().unwrap();
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
        {
            let mut txn = loro.txn().unwrap();
            text.insert_with_txn(&mut txn, 0, "123").unwrap();
            txn.commit().unwrap();
        }
        assert_eq!(count.load(Ordering::SeqCst), 2);
        loro.unsubscribe(sub);
        {
            let mut txn = loro.txn().unwrap();
            text.insert_with_txn(&mut txn, 0, "123").unwrap();
            txn.commit().unwrap();
        }
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
}
