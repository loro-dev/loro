use crate::sync::Mutex;
use bytes::Bytes;
use loro_kv_store::{mem_store::MemKvConfig, MemKvStore};
use std::{collections::BTreeSet, ops::Bound, sync::Arc};

use crate::kv_store::KvStore;

/// This thin wrapper aims to limit the ability to modify the kv store and make
/// it easy to find all the modifications.
pub(crate) struct KvWrapper {
    kv: Arc<Mutex<dyn KvStore>>,
}

impl Clone for KvWrapper {
    /// Deep clone the inner kv store.
    fn clone(&self) -> Self {
        Self {
            kv: self.kv.lock().clone_store(),
        }
    }
}

impl KvWrapper {
    pub fn new_mem() -> Self {
        Self {
            // kv: Arc::new(Mutex::new(BTreeMap::new())),
            kv: Arc::new(Mutex::new(MemKvStore::new(
                // set false because it's depended by GC snapshot's import & export
                MemKvConfig::default().should_encode_none(false),
            ))),
        }
    }

    pub fn arc_clone(&self) -> Self {
        Self {
            kv: self.kv.clone(),
        }
    }

    pub fn import(&self, bytes: Bytes) -> Result<(), String> {
        let mut kv = self.kv.lock();
        kv.import_all(bytes)
    }

    pub fn export(&self) -> Bytes {
        let mut kv = self.kv.lock();
        kv.export_all()
    }

    pub fn get(&self, key: &[u8]) -> Option<Bytes> {
        let kv = self.kv.lock();
        kv.get(key)
    }

    pub fn with_kv<R>(&self, f: impl FnOnce(&dyn KvStore) -> R) -> R {
        let kv = self.kv.lock();
        f(&*kv)
    }

    pub(crate) fn keys(&self) -> BTreeSet<Vec<u8>> {
        let kv = self.kv.lock();
        kv.scan(Bound::Unbounded, Bound::Unbounded)
            .map(|(k, _)| k.to_vec())
            .collect()
    }

    pub fn set_all(&self, updates: Vec<(Bytes, Bytes)>) {
        let mut kv = self.kv.lock();
        for (k, v) in updates {
            kv.set(&k, v);
        }
    }

    #[allow(unused)]
    pub(crate) fn contains_key(&self, key: &[u8]) -> bool {
        self.kv.lock().contains_key(key)
    }

    pub(crate) fn remove_same(&self, old_kv: &KvWrapper) {
        let old_entries = {
            let other = old_kv.kv.lock();
            other
                .scan(Bound::Unbounded, Bound::Unbounded)
                .collect::<Vec<_>>()
        };
        let mut this = self.kv.lock();
        for (k, v) in old_entries {
            if this.get(&k) == Some(v) {
                this.remove(&k);
            }
        }
    }

    pub(crate) fn remove(&self, k: &[u8]) -> Option<Bytes> {
        self.kv.lock().remove(k)
    }

    /// Remove all keys not in the given set
    pub(crate) fn retain_keys(&self, keys: &BTreeSet<Vec<u8>>) {
        let mut kv = self.kv.lock();
        let mut to_remove = BTreeSet::new();
        for (k, _) in kv.scan(Bound::Unbounded, Bound::Unbounded) {
            if !keys.contains(&*k) {
                to_remove.insert(k);
            }
        }

        for k in to_remove {
            kv.remove(&k);
        }
    }

    pub(crate) fn insert(&self, k: &[u8], v: Bytes) {
        self.kv.lock().set(k, v);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.kv.lock().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        panic::{catch_unwind, AssertUnwindSafe},
        sync::mpsc,
        time::Duration,
    };

    fn assert_completes_without_deadlock(name: &'static str, f: impl FnOnce() + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(f));
            let _ = tx.send(result);
        });

        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => {}
            Ok(Err(payload)) => std::panic::resume_unwind(payload),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                panic!("{name} did not complete; possible recursive KV lock deadlock")
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!("{name} thread disconnected without a result")
            }
        }
    }

    #[test]
    fn set_all_applies_precollected_updates() {
        let kv = KvWrapper::new_mem();

        assert_completes_without_deadlock("set_all", move || {
            kv.set_all(vec![(
                Bytes::from_static(b"key"),
                Bytes::from_static(b"value"),
            )]);
            assert_eq!(kv.get(b"key"), Some(Bytes::from_static(b"value")));
        });
    }

    #[test]
    fn remove_same_can_compare_a_store_with_itself() {
        let kv = KvWrapper::new_mem();
        kv.insert(b"key", Bytes::from_static(b"value"));

        assert_completes_without_deadlock("remove_same", move || {
            kv.remove_same(&kv);
            assert_eq!(kv.get(b"key"), None);
        });
    }
}
