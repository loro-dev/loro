use std::{
    collections::BTreeMap,
    ops::Bound,
    sync::{Arc, Mutex},
};

use bytes::Bytes;
use fxhash::FxHashMap;
use loro_common::ContainerID;

use crate::kv_store::KvStore;

pub(crate) enum Status {
    BytesOnly,
    ImmBoth,
    MutState,
}

/// This thin wrapper aims to limit the ability to modify the kv store and make
/// it easy to find all the modifications.
pub(crate) struct KvWrapper {
    kv: Arc<Mutex<dyn KvStore>>,
}

impl Clone for KvWrapper {
    fn clone(&self) -> Self {
        Self {
            kv: self.kv.lock().unwrap().clone_store(),
        }
    }
}

impl KvWrapper {
    pub fn new_mem() -> Self {
        Self {
            kv: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn import(&self, bytes: Bytes) {
        let mut kv = self.kv.lock().unwrap();
        assert!(kv.len() == 0);
        kv.import_all(bytes);
    }

    pub fn export(&self) -> Bytes {
        let kv = self.kv.lock().unwrap();
        kv.export_all()
    }

    pub fn get(&self, key: &[u8]) -> Option<Bytes> {
        let kv = self.kv.lock().unwrap();
        kv.get(key)
    }

    pub fn with_kv<R>(&self, f: impl FnOnce(&dyn KvStore) -> R) -> R {
        let kv = self.kv.lock().unwrap();
        f(&*kv)
    }

    pub fn set_all(&self, iter: impl Iterator<Item = (Bytes, Bytes)>) {
        let mut kv = self.kv.lock().unwrap();
        for (k, v) in iter {
            kv.set(&k, v);
        }
    }

    pub(crate) fn contains_key(&self, key: &[u8]) -> bool {
        self.kv.lock().unwrap().contains_key(key)
    }
}
