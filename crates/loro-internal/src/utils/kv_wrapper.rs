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
        // Only reached while decoding a snapshot/state blob whose integrity is
        // already guaranteed by the document-level checksum in
        // `parse_header_and_body(.., true)`, so skip the redundant per-block
        // checksum.
        kv.import_all_unchecked(bytes)
    }

    pub fn export(&self) -> Bytes {
        let mut kv = self.kv.lock();
        kv.export_all()
    }

    pub fn get(&self, key: &[u8]) -> Option<Bytes> {
        let kv = self.kv.lock();
        kv.get(key)
    }

    pub(crate) fn keys(&self) -> BTreeSet<Vec<u8>> {
        let kv = self.kv.lock();
        kv.scan(Bound::Unbounded, Bound::Unbounded)
            .map(|(k, _)| k.to_vec())
            .collect()
    }

    /// Snapshot all entries while holding only the KV lock.
    ///
    /// Callers that also need the arena lock must use this instead of entering arena code from
    /// inside a KV-locked closure. Lazy arena parent resolution can acquire this same KV lock.
    pub(crate) fn scan_all_entries(&self) -> Vec<(Bytes, Bytes)> {
        let kv = self.kv.lock();
        kv.scan(Bound::Unbounded, Bound::Unbounded).collect()
    }

    pub(crate) fn scan_all_keys(&self) -> Vec<Bytes> {
        let kv = self.kv.lock();
        kv.scan(Bound::Unbounded, Bound::Unbounded)
            .map(|(k, _)| k)
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
        if Arc::ptr_eq(&self.kv, &old_kv.kv) {
            let mut this = self.kv.lock();
            let keys = this
                .scan(Bound::Unbounded, Bound::Unbounded)
                .map(|(k, _)| k)
                .collect::<Vec<_>>();
            for k in keys {
                this.remove(&k);
            }
            return;
        }

        let other = old_kv.kv.lock();
        let mut this = self.kv.lock();
        for (k, v) in other.scan(Bound::Unbounded, Bound::Unbounded) {
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

    #[test]
    fn set_all_applies_precollected_updates() {
        let kv = KvWrapper::new_mem();

        kv.set_all(vec![(
            Bytes::from_static(b"key"),
            Bytes::from_static(b"value"),
        )]);
        assert_eq!(kv.get(b"key"), Some(Bytes::from_static(b"value")));
    }

    #[test]
    fn remove_same_streams_different_stores() {
        let kv = KvWrapper::new_mem();
        let old_kv = KvWrapper::new_mem();
        kv.insert(b"same", Bytes::from_static(b"value"));
        kv.insert(b"changed", Bytes::from_static(b"new"));
        old_kv.insert(b"same", Bytes::from_static(b"value"));
        old_kv.insert(b"changed", Bytes::from_static(b"old"));

        kv.remove_same(&old_kv);

        assert_eq!(kv.get(b"same"), None);
        assert_eq!(kv.get(b"changed"), Some(Bytes::from_static(b"new")));
    }
}
