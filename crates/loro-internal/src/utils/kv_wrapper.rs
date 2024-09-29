use std::{
    collections::BTreeSet,
    ops::Bound,
    sync::{Arc, Mutex},
};

use bytes::Bytes;
use loro_kv_store::{mem_store::MemKvConfig, MemKvStore};

use crate::kv_store::KvStore;

/// This thin wrapper aims to limit the ability to modify the kv store and make
/// it easy to find all the modifications.
pub(crate) struct KvWrapper {
    kv: Arc<Mutex<dyn KvStore>>,
}

impl Clone for KvWrapper {
    fn clone(&self) -> Self {
        Self {
            kv: self.kv.try_lock().unwrap().clone_store(),
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

    pub fn import(&self, bytes: Bytes) {
        let mut kv = self.kv.try_lock().unwrap();
        kv.import_all(bytes).unwrap();
    }

    pub fn export(&self) -> Bytes {
        let mut kv = self.kv.try_lock().unwrap();
        kv.export_all()
    }

    pub fn get(&self, key: &[u8]) -> Option<Bytes> {
        let kv = self.kv.try_lock().unwrap();
        kv.get(key)
    }

    pub fn with_kv<R>(&self, f: impl FnOnce(&dyn KvStore) -> R) -> R {
        let kv = self.kv.try_lock().unwrap();
        f(&*kv)
    }

    pub fn set_all(&self, iter: impl Iterator<Item = (Bytes, Bytes)>) {
        let mut kv = self.kv.try_lock().unwrap();
        for (k, v) in iter {
            kv.set(&k, v);
        }
    }

    pub(crate) fn contains_key(&self, key: &[u8]) -> bool {
        self.kv.try_lock().unwrap().contains_key(key)
    }

    pub(crate) fn remove_same(&self, old_kv: &KvWrapper) {
        let other = old_kv.kv.try_lock().unwrap();
        let mut this = self.kv.try_lock().unwrap();
        for (k, v) in other.scan(Bound::Unbounded, Bound::Unbounded) {
            if this.get(&k) == Some(v) {
                this.remove(&k);
            }
        }
    }

    pub(crate) fn remove(&self, k: &[u8]) -> Option<Bytes> {
        self.kv.try_lock().unwrap().remove(k)
    }

    /// Remove all keys not in the given set
    pub(crate) fn retain_keys(&self, keys: &BTreeSet<Vec<u8>>) {
        let mut kv = self.kv.try_lock().unwrap();
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
        self.kv.try_lock().unwrap().set(k, v);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.kv.try_lock().unwrap().is_empty()
    }
}
