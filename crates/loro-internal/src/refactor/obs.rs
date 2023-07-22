use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use fxhash::{FxHashMap, FxHashSet};
use loro_common::ContainerID;

use crate::container::registry::ContainerIdx;

use super::{
    arena::SharedArena,
    event::{DiffEvent, DocDiff},
};

pub type Subscriber = Arc<dyn for<'a> Fn(DiffEvent<'a>)>;

#[derive(Default)]
struct ObserverInner {
    subscribers: FxHashMap<SubID, Subscriber>,
    containers: FxHashMap<ContainerIdx, FxHashSet<SubID>>,
    root: FxHashSet<SubID>,
    deleted: FxHashSet<SubID>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SubID(usize);

pub struct Observer {
    inner: Mutex<ObserverInner>,
    arena: SharedArena,
    next_sub_id: AtomicUsize,
    taken_times: AtomicUsize,
}

impl Observer {
    pub fn new(arena: SharedArena) -> Self {
        Self {
            arena,
            next_sub_id: AtomicUsize::new(0),
            taken_times: AtomicUsize::new(0),
            inner: Mutex::new(ObserverInner {
                subscribers: Default::default(),
                containers: Default::default(),
                root: Default::default(),
                deleted: Default::default(),
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

    pub fn subscribe_deep(&self, callback: Subscriber) -> SubID {
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

    pub(crate) fn emit(&self, doc_diff: &DocDiff) {
        if self.taken_times.load(Ordering::Relaxed) > 0 {
            // WARNING: recursive emit
            return;
        }

        let mut inner = self.take_inner();

        for container_diff in doc_diff.diff.iter() {
            self.arena.with_ancestors(container_diff.idx, |ancestor| {
                if let Some(subs) = inner.containers.get_mut(&ancestor) {
                    subs.retain(|sub| match inner.subscribers.get_mut(sub) {
                        Some(f) => {
                            f(DiffEvent {
                                container: container_diff,
                                doc: doc_diff,
                            });
                            true
                        }
                        None => false,
                    });
                }
            });

            inner
                .root
                .retain(|sub| match inner.subscribers.get_mut(sub) {
                    Some(f) => {
                        f(DiffEvent {
                            container: container_diff,
                            doc: doc_diff,
                        });
                        true
                    }
                    None => false,
                });
        }

        self.reset_inner(inner);
    }

    fn take_inner(&self) -> ObserverInner {
        self.taken_times
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut inner_guard = self.inner.lock().unwrap();
        std::mem::take(&mut *inner_guard)
    }

    fn reset_inner(&self, mut inner: ObserverInner) {
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

        *inner_guard = inner;
        self.taken_times
            .fetch_sub(1, std::sync::atomic::Ordering::Release);
    }

    pub fn unsubscribe(&mut self, sub_id: SubID) {
        let mut inner = self.inner.lock().unwrap();
        inner.subscribers.remove(&sub_id);
        if self.is_taken() {
            inner.deleted.insert(sub_id);
        }
    }

    fn is_taken(&self) -> bool {
        self.taken_times.load(std::sync::atomic::Ordering::Acquire) != 0
    }
}
