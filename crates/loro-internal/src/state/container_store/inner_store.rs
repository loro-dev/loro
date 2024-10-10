use crate::{
    arena::SharedArena, configure::Configure, container::idx::ContainerIdx,
    state::container_store::FRONTIERS_KEY, utils::kv_wrapper::KvWrapper, version::Frontiers,
};
use bytes::Bytes;
use fxhash::FxHashMap;
use loro_common::ContainerID;
use std::ops::Bound;

use super::ContainerWrapper;

/// The invariants about this struct:
///
/// - `len` is the number of containers in the store. If a container is in both kv and store,
///   it should only take 1 space in `len`.
/// - `kv` is either the same or older than `store`.
/// - if `all_loaded` is true, then `store` contains all the entries from `kv`
pub(crate) struct InnerStore {
    arena: SharedArena,
    store: FxHashMap<ContainerIdx, ContainerWrapper>,
    kv: KvWrapper,
    len: usize,
    all_loaded: bool,
}

impl std::fmt::Debug for InnerStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InnerStore").finish()
    }
}

/// This impl block contains all the mutation code that may break the invariants of this struct
impl InnerStore {
    pub(super) fn get_or_insert_with(
        &mut self,
        idx: ContainerIdx,
        f: impl FnOnce() -> ContainerWrapper,
    ) -> &mut ContainerWrapper {
        if let std::collections::hash_map::Entry::Vacant(e) = self.store.entry(idx) {
            let id = self.arena.get_container_id(idx).unwrap();
            let key = id.to_bytes();
            if !self.all_loaded {
                if let Some(v) = self.kv.get(&key) {
                    let c = ContainerWrapper::new_from_bytes(v);
                    e.insert(c);
                    return self.store.get_mut(&idx).unwrap();
                }
            }
            let c = f();
            e.insert(c);
            self.len += 1;
        }

        self.store.get_mut(&idx).unwrap()
    }

    pub(super) fn ensure_container(
        &mut self,
        idx: ContainerIdx,
        f: impl FnOnce() -> ContainerWrapper,
    ) {
        if self.store.contains_key(&idx) {
            return;
        }

        let c = f();
        self.store.insert(idx, c);
        self.len += 1;
    }

    pub(crate) fn get_mut(&mut self, idx: ContainerIdx) -> Option<&mut ContainerWrapper> {
        if let std::collections::hash_map::Entry::Vacant(e) = self.store.entry(idx) {
            let id = self.arena.get_container_id(idx).unwrap();
            let key = id.to_bytes();
            if !self.all_loaded {
                if let Some(v) = self.kv.get(&key) {
                    let c = ContainerWrapper::new_from_bytes(v);
                    e.insert(c);
                }
            }
        }

        self.store.get_mut(&idx)
    }

    pub(crate) fn iter_all_containers_mut(
        &mut self,
    ) -> impl Iterator<Item = (&ContainerIdx, &mut ContainerWrapper)> {
        self.load_all();
        self.store.iter_mut()
    }

    pub(crate) fn iter_all_container_ids(&mut self) -> impl Iterator<Item = ContainerID> + '_ {
        // PERF: we don't need to load all the containers here
        self.load_all();
        self.store
            .keys()
            .map(|idx| self.arena.get_container_id(*idx).unwrap())
    }

    pub(crate) fn encode(&mut self) -> Bytes {
        self.flush();
        self.kv.export()
    }

    pub(crate) fn flush(&mut self) {
        self.kv
            .set_all(self.store.iter_mut().filter_map(|(idx, c)| {
                if c.is_flushed() {
                    return None;
                }

                let cid = self.arena.get_container_id(*idx).unwrap();
                let cid: Bytes = cid.to_bytes().into();
                let value = c.encode();
                c.set_flushed(true);
                // println!("cid.len = {} value.len = {}", cid.len(), value.len());
                Some((cid, value))
            }));
    }

    pub(crate) fn get_kv(&self) -> &KvWrapper {
        &self.kv
    }

    pub(crate) fn decode(
        &mut self,
        bytes: bytes::Bytes,
    ) -> Result<Option<Frontiers>, loro_common::LoroError> {
        assert!(self.kv.is_empty());
        assert_eq!(self.len, self.store.len());
        let mut fr = None;
        self.kv.import(bytes);
        if let Some(f) = self.kv.remove(FRONTIERS_KEY) {
            fr = Some(Frontiers::decode(&f)?);
        }

        self.kv.with_kv(|kv| {
            let mut count = self.len;
            let iter = kv.scan(Bound::Unbounded, Bound::Unbounded);
            for (k, v) in iter {
                count += 1;
                let cid = ContainerID::from_bytes(&k);
                let c = ContainerWrapper::new_from_bytes(v);
                let parent = c.parent();
                let idx = self.arena.register_container(&cid);
                let p = parent.as_ref().map(|p| self.arena.register_container(p));
                self.arena.set_parent(idx, p);
                if self.store.insert(idx, c).is_some() {
                    count -= 1;
                }
            }

            self.len = count;
        });

        self.all_loaded = true;
        Ok(fr)
    }

    pub(crate) fn decode_twice(
        &mut self,
        bytes_a: bytes::Bytes,
        bytes_b: bytes::Bytes,
    ) -> Result<(), loro_common::LoroError> {
        assert!(self.kv.is_empty());
        assert_eq!(self.len, self.store.len());
        // TODO: add assert that all containers in the store should be empty right now
        self.kv.import(bytes_a);
        self.kv.import(bytes_b);
        self.kv.remove(FRONTIERS_KEY);
        self.kv.with_kv(|kv| {
            let mut count = self.len;
            let iter = kv.scan(Bound::Unbounded, Bound::Unbounded);
            for (k, v) in iter {
                count += 1;
                let cid = ContainerID::from_bytes(&k);
                let c = ContainerWrapper::new_from_bytes(v);
                let parent = c.parent();
                let idx = self.arena.register_container(&cid);
                let p = parent.as_ref().map(|p| self.arena.register_container(p));
                self.arena.set_parent(idx, p);
                if self.store.insert(idx, c).is_some() {
                    count -= 1;
                }
            }

            self.len = count;
        });

        self.all_loaded = true;
        Ok(())
    }

    fn load_all(&mut self) {
        if self.all_loaded {
            return;
        }

        self.kv.with_kv(|kv| {
            let iter = kv.scan(Bound::Unbounded, Bound::Unbounded);
            for (k, v) in iter {
                let cid = ContainerID::from_bytes(&k);
                let idx = self.arena.register_container(&cid);
                if self.store.contains_key(&idx) {
                    // the container is already loaded
                    // the content in `store` is guaranteed to be newer than the content in `kv`
                    continue;
                }

                let container = ContainerWrapper::new_from_bytes(v);
                self.store.insert(idx, container);
            }
        });

        self.all_loaded = true;
    }

    pub(crate) fn can_import_snapshot(&self) -> bool {
        if !self.kv.is_empty() {
            return false;
        }

        self.store.iter().all(|(_, c)| c.is_state_empty())
    }
}

impl InnerStore {
    pub(crate) fn new(arena: SharedArena) -> Self {
        Self {
            arena,
            store: FxHashMap::default(),
            kv: KvWrapper::new_mem(),
            len: 0,
            all_loaded: true,
        }
    }

    pub(crate) fn fork(&mut self, arena: SharedArena, _config: &Configure) -> InnerStore {
        // PERF: we can try to reuse
        let bytes = self.encode();
        let mut new_store = Self::new(arena);
        new_store.decode(bytes).unwrap();
        new_store
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn estimate_size(&self) -> usize {
        self.kv.with_kv(|kv| kv.size())
            + self
                .store
                .values()
                .map(|c| if c.is_flushed() { 0 } else { c.estimate_size() })
                .sum::<usize>()
    }
}
