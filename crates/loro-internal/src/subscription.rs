use super::{
    arena::SharedArena,
    event::{DiffEvent, DocDiff},
};
use crate::{
    container::idx::ContainerIdx, utils::subscription::SubscriberSet, ContainerDiff, LoroDoc,
    Subscription,
};
use fxhash::FxHashMap;
use loro_common::{ContainerID, InternalString, ID};
use smallvec::SmallVec;
use std::{collections::VecDeque, sync::Arc};

use crate::sync::Mutex;
/// The callback of the local update.
pub type LocalUpdateCallback = Box<dyn Fn(&Vec<u8>) -> bool + Send + Sync + 'static>;
/// The callback of the peer id change. The second argument is the next counter for the peer.
pub type PeerIdUpdateCallback = Box<dyn Fn(&ID) -> bool + Send + Sync + 'static>;
/// The callback for undo diff batch generation.
pub type UndoCallback = Box<dyn Fn(&UndoCallbackArgs) -> bool + Send + Sync + 'static>;
pub type Subscriber = Arc<dyn (for<'a> Fn(DiffEvent<'a>)) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct UndoCallbackArgs {
    pub diff: crate::undo::DiffBatch,
    pub origin: InternalString,
}

impl LoroDoc {
    /// Subscribe to the changes of the peer id.
    pub fn subscribe_peer_id_change(&self, callback: PeerIdUpdateCallback) -> Subscription {
        let (s, enable) = self.peer_id_change_subs.inner().insert((), callback);
        enable();
        s
    }

    /// Subscribe to undo diff batches generated during local operations.
    /// This is an internal API used by the UndoManager.
    #[doc(hidden)]
    pub fn subscribe_undo_diffs(&self, callback: UndoCallback) -> Subscription {
        let (s, enable) = self.inner.undo_subs.inner().insert((), callback);
        enable();
        s
    }
}

struct ObserverInner {
    subscriber_set: SubscriberSet<Option<ContainerIdx>, Subscriber>,
    queue: Arc<Mutex<VecDeque<DocDiff>>>,
}

impl Default for ObserverInner {
    fn default() -> Self {
        Self {
            subscriber_set: SubscriberSet::new(),
            queue: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

pub struct Observer {
    inner: ObserverInner,
    arena: SharedArena,
}

impl std::fmt::Debug for Observer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Observer").finish()
    }
}

impl Observer {
    pub fn new(arena: SharedArena) -> Self {
        Self {
            arena,
            inner: ObserverInner::default(),
        }
    }

    pub fn subscribe(&self, id: &ContainerID, callback: Subscriber) -> Subscription {
        let idx = self.arena.register_container(id);
        let inner = &self.inner;
        let (sub, enable) = inner.subscriber_set.insert(Some(idx), callback);
        enable();
        sub
    }

    pub fn subscribe_root(&self, callback: Subscriber) -> Subscription {
        let inner = &self.inner;
        let (sub, enable) = inner.subscriber_set.insert(None, callback);
        enable();
        sub
    }

    pub(crate) fn emit(&self, doc_diff: DocDiff) {
        let success = self.emit_inner(doc_diff);
        if success {
            let mut e = self.inner.queue.lock().unwrap().pop_front();
            while let Some(event) = e {
                self.emit_inner(event);
                e = self.inner.queue.lock().unwrap().pop_front();
            }
        }
    }

    // When emitting changes, we need to make sure that the observer is not locked.
    fn emit_inner(&self, doc_diff: DocDiff) -> bool {
        let inner = &self.inner;
        let mut container_events_map: FxHashMap<ContainerIdx, SmallVec<[&ContainerDiff; 1]>> =
            Default::default();
        for container_diff in doc_diff.diff.iter() {
            self.arena
                .with_ancestors(container_diff.idx, |ancestor, _| {
                    if inner.subscriber_set.may_include(&Some(ancestor)) {
                        container_events_map
                            .entry(ancestor)
                            .or_default()
                            .push(container_diff);
                    }
                });
        }

        {
            // Check whether we are calling events recursively.
            // If so, push the event to the queue
            if inner.subscriber_set.is_recursive_calling(&None)
                || container_events_map
                    .keys()
                    .any(|x| inner.subscriber_set.is_recursive_calling(&Some(*x)))
            {
                drop(container_events_map);
                inner.queue.lock().unwrap().push_back(doc_diff);
                return false;
            }
        }

        for (container_idx, container_diffs) in container_events_map {
            inner
                .subscriber_set
                .retain(&Some(container_idx), &mut |callback| {
                    (callback)(DiffEvent {
                        current_target: Some(self.arena.get_container_id(container_idx).unwrap()),
                        events: &container_diffs,
                        event_meta: &doc_diff,
                    });
                    true
                })
                .unwrap();
        }

        let events: Vec<_> = doc_diff.diff.iter().collect();
        inner
            .subscriber_set
            .retain(&None, &mut |callback| {
                (callback)(DiffEvent {
                    current_target: None,
                    events: &events,
                    event_meta: &doc_diff,
                });
                true
            })
            .unwrap();

        true
    }
}

#[cfg(test)]
mod test {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::{handler::HandlerTrait, LoroDoc};

    #[test]
    fn test_recursive_events() {
        let loro = Arc::new(LoroDoc::new());
        let loro_cp = loro.clone();
        let count = Arc::new(AtomicUsize::new(0));
        let count_cp = Arc::clone(&count);
        let _g = loro_cp.subscribe_root(Arc::new(move |_| {
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
        sub.unsubscribe();
        {
            let mut txn = loro.txn().unwrap();
            text.insert_with_txn(&mut txn, 0, "123").unwrap();
            txn.commit().unwrap();
        }
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
}
