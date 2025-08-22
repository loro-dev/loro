use super::{ContainerCreationContext, State};
use crate::arena::LoadAllFlag;
use crate::sync::{AtomicU64, Mutex};
use crate::{
    arena::SharedArena, configure::Configure, container::idx::ContainerIdx,
    utils::kv_wrapper::KvWrapper, version::Frontiers,
};
use bytes::Bytes;
use inner_store::InnerStore;
use loro_common::{ContainerID, LoroResult, LoroValue};
use std::sync::Arc;

pub(crate) use container_wrapper::ContainerWrapper;

mod container_wrapper;
mod inner_store;

/// Encoding Schema for Container Store
///
/// KV-Store:
/// - Key: Encoded Container ID
/// - Value: Encoded Container State
///
/// ─ ─ ─ ─ ─ ─ ─ For Each Container Type ─ ─ ─ ─ ─ ─ ─ ─
///
/// ┌────────────────┬──────────────────────────────────┐
/// │   u16 Depth    │             ParentID             │
/// └────────────────┴──────────────────────────────────┘
/// ┌───────────────────────────────────────────────────┐
/// │ ┌───────────────────────────────────────────────┐ │
/// │ │                Into<LoroValue>                │ │
/// │ └───────────────────────────────────────────────┘ │
/// │                                                   │
/// │             Container Specific Encode             │
/// │                                                   │
/// │                                                   │
/// │                                                   │
/// │                                                   │
/// └───────────────────────────────────────────────────┘
pub(crate) struct ContainerStore {
    arena: SharedArena,
    store: InnerStore,
    shallow_root_store: Option<Arc<GcStore>>,
    conf: Configure,
    peer: Arc<AtomicU64>,
}

pub(crate) const FRONTIERS_KEY: &[u8] = b"fr";
impl std::fmt::Debug for ContainerStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContainerStore")
            .field("store", &self.store)
            .finish()
    }
}

#[derive(Debug)]
pub(crate) struct GcStore {
    pub shallow_root_frontiers: Frontiers,
    pub store: Mutex<InnerStore>,
}

macro_rules! ctx {
    ($self:expr) => {
        ContainerCreationContext {
            configure: &$self.conf,
            peer: $self.peer.load(std::sync::atomic::Ordering::Relaxed),
        }
    };
}

impl ContainerStore {
    pub fn new(arena: SharedArena, conf: Configure, peer: Arc<AtomicU64>) -> Self {
        ContainerStore {
            store: InnerStore::new(arena.clone(), conf.clone()),
            arena,
            conf,
            shallow_root_store: None,
            peer,
        }
    }

    pub fn can_import_snapshot(&self) -> bool {
        if self.shallow_root_store.is_some() {
            return false;
        }

        self.store.can_import_snapshot()
    }

    pub fn get_container_mut(&mut self, idx: ContainerIdx) -> Option<&mut State> {
        self.store
            .get_mut(idx)
            .map(|x| x.get_state_mut(idx, ctx!(self)))
    }

    #[allow(unused)]
    pub fn get_container(&mut self, idx: ContainerIdx) -> Option<&State> {
        self.store
            .get_mut(idx)
            .map(|x| x.get_state(idx, ctx!(self)))
    }

    pub fn shallow_root_store(&self) -> Option<&Arc<GcStore>> {
        self.shallow_root_store.as_ref()
    }

    pub fn get_value(&mut self, idx: ContainerIdx) -> Option<LoroValue> {
        self.store
            .get_mut(idx)
            .map(|c| c.get_value(idx, ctx!(self)))
    }

    pub fn encode(&mut self) -> Bytes {
        self.store.encode()
    }

    pub(crate) fn flush(&mut self) {
        self.store.flush()
    }

    pub fn shallow_root_frontiers(&self) -> Option<&Frontiers> {
        self.shallow_root_store
            .as_ref()
            .map(|x| &x.shallow_root_frontiers)
    }

    pub(crate) fn decode(&mut self, bytes: Bytes) -> LoroResult<Option<Frontiers>> {
        self.store.decode(bytes)
    }

    pub(crate) fn decode_gc(
        &mut self,
        shallow_bytes: Bytes,
        start_frontiers: Frontiers,
        config: Configure,
    ) -> LoroResult<Option<Frontiers>> {
        assert!(self.shallow_root_store.is_none());
        let mut inner = InnerStore::new(self.arena.clone(), config);
        let f = inner.decode(shallow_bytes)?;
        self.shallow_root_store = Some(Arc::new(GcStore {
            shallow_root_frontiers: start_frontiers,
            store: Mutex::new(inner),
        }));
        Ok(f)
    }

    pub(crate) fn decode_state_by_two_bytes(
        &mut self,
        shallow_bytes: Bytes,
        state_bytes: Bytes,
    ) -> LoroResult<()> {
        self.store
            .decode_twice(shallow_bytes.clone(), state_bytes)?;
        Ok(())
    }

    pub fn iter_and_decode_all(&mut self) -> impl Iterator<Item = &mut State> {
        self.store.iter_all_containers_mut().map(|(idx, v)| {
            v.get_state_mut(
                *idx,
                ContainerCreationContext {
                    configure: &self.conf,
                    peer: self.peer.load(std::sync::atomic::Ordering::Relaxed),
                },
            )
        })
    }

    pub fn get_kv(&self) -> &KvWrapper {
        self.store.get_kv()
    }

    pub fn iter_all_containers(
        &mut self,
    ) -> impl Iterator<Item = (&ContainerIdx, &mut ContainerWrapper)> {
        self.store.iter_all_containers_mut()
    }

    pub fn iter_all_container_ids(&mut self) -> impl Iterator<Item = ContainerID> + '_ {
        self.store.iter_all_container_ids()
    }

    pub fn load_all(&mut self) -> LoadAllFlag {
        self.store.load_all();
        LoadAllFlag
    }

    pub(super) fn get_or_create_mut(&mut self, idx: ContainerIdx) -> &mut State {
        self.store
            .get_or_insert_with(idx, || {
                let state = super::create_state_(
                    idx,
                    &self.conf,
                    self.peer.load(std::sync::atomic::Ordering::Relaxed),
                );
                ContainerWrapper::new(state, &self.arena)
            })
            .get_state_mut(idx, ctx!(self))
    }

    pub(crate) fn ensure_container(&mut self, id: &loro_common::ContainerID) {
        let idx = self.arena.register_container(id);
        self.store.ensure_container(idx, || {
            let state = super::create_state_(
                idx,
                &self.conf,
                self.peer.load(std::sync::atomic::Ordering::Relaxed),
            );
            ContainerWrapper::new(state, &self.arena)
        });
    }

    pub(super) fn get_or_create_imm(&mut self, idx: ContainerIdx) -> &State {
        self.store
            .get_or_insert_with(idx, || {
                let state = super::create_state_(
                    idx,
                    &self.conf,
                    self.peer.load(std::sync::atomic::Ordering::Relaxed),
                );
                ContainerWrapper::new(state, &self.arena)
            })
            .get_state(idx, ctx!(self))
    }

    pub(crate) fn estimate_size(&self) -> usize {
        self.store.estimate_size()
    }

    pub(crate) fn fork(
        &mut self,
        arena: SharedArena,
        peer: Arc<AtomicU64>,
        config: Configure,
    ) -> ContainerStore {
        ContainerStore {
            store: self.store.fork(arena.clone(), &config),
            arena,
            conf: config,
            peer,
            shallow_root_store: None,
        }
    }

    #[allow(unused)]
    fn check_eq_after_parsing(&mut self, other: &mut ContainerStore) {
        for (idx, container) in self.store.iter_all_containers_mut() {
            let id = self.arena.get_container_id(*idx).unwrap();
            let other_idx = other.arena.register_container(&id);
            let other_container = other
                .store
                .get_mut(other_idx)
                .expect("container not found on other store");
            let other_id = other.arena.get_container_id(other_idx).unwrap();
            assert_eq!(
                id, other_id,
                "container id mismatch {:?} {:?}",
                id, other_id
            );
            assert_eq!(
                container.get_value(*idx, ctx!(self)),
                other_container.get_value(other_idx, ctx!(other)),
                "value mismatch"
            );

            if container.encode() != other_container.encode() {
                panic!(
                    "container mismatch Origin: {:#?}, New: {:#?}",
                    &container, &other_container
                );
            }

            other_container
                .decode_state(other_idx, ctx!(other))
                .unwrap();
            other_container.clear_bytes();
            if container.encode() != other_container.encode() {
                panic!(
                    "container mismatch Origin: {:#?}, New: {:#?}",
                    &container, &other_container
                );
            }
        }
    }
}

mod encode {
    use serde::{Deserialize, Serialize};
    use std::borrow::Cow;

    #[derive(Serialize, Deserialize)]
    struct EncodedStateStore<'a> {
        #[serde(borrow)]
        cids: Cow<'a, [u8]>,
        #[serde(borrow)]
        bytes: Cow<'a, [u8]>,
    }

    /// ContainerID is sorted by IsRoot, ContainerType, PeerID, Counter
    ///
    /// ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─For CIDs ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─
    ///
    /// ┌───────────────────────────────────────────────────┐
    /// │                  Container Types                  │
    /// └───────────────────────────────────────────────────┘
    /// ┌───────────────────────────────────────────────────┐
    /// │                      IsRoot                       │
    /// └───────────────────────────────────────────────────┘
    /// ┌───────────────────────────────────────────────────┐
    /// │              Root Container Strings               │
    /// └───────────────────────────────────────────────────┘
    /// ┌───────────────────────────────────────────────────┐
    /// │              NormalContainer PeerIDs              │
    /// └───────────────────────────────────────────────────┘
    /// ┌───────────────────────────────────────────────────┐
    /// │             NormalContainer Counters              │
    /// └───────────────────────────────────────────────────┘
    /// ┌───────────────────────────────────────────────────┐
    /// │                    Offsets                        │
    /// └───────────────────────────────────────────────────┘

    #[derive(Serialize, Deserialize)]
    struct EncodedCid<'a> {
        #[serde(borrow)]
        types: Cow<'a, [u8]>,
        is_root_bools: Cow<'a, [u8]>,
        strings: Cow<'a, [u8]>,
        peer_ids: Cow<'a, [u8]>,
        counters: Cow<'a, [u8]>,
        offsets: Cow<'a, [u8]>,
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{state::TreeParentId, ListHandler, LoroDoc, MapHandler, MovableListHandler};

    fn decode_container_store(bytes: Bytes) -> ContainerStore {
        let mut new_store = ContainerStore::new(
            SharedArena::new(),
            Configure::default(),
            Arc::new(AtomicU64::new(233)),
        );

        new_store.decode(bytes).unwrap();
        new_store
    }

    fn init_doc() -> LoroDoc {
        let doc = LoroDoc::new();
        doc.start_auto_commit();
        let text = doc.get_text("text");
        text.insert(0, "hello").unwrap();
        let map = doc.get_map("map");
        map.insert("key", "value").unwrap();

        let list = doc.get_list("list");
        list.push("item1").unwrap();

        let tree = doc.get_tree("tree");
        let root = tree.create(TreeParentId::Root).unwrap();
        tree.create_at(TreeParentId::Node(root), 0).unwrap();

        let movable_list = doc.get_movable_list("movable_list");
        movable_list.insert(0, "movable_item").unwrap();

        // Create child containers
        let child_map = map
            .insert_container("child_map", MapHandler::new_detached())
            .unwrap();
        child_map.insert("child_key", "child_value").unwrap();

        let child_list = list
            .insert_container(0, ListHandler::new_detached())
            .unwrap();
        child_list.push("child_item").unwrap();
        let child_movable_list = movable_list
            .insert_container(0, MovableListHandler::new_detached())
            .unwrap();
        child_movable_list.insert(0, "child_movable_item").unwrap();
        doc
    }

    #[test]
    fn test_container_store_exports_imports() {
        let doc = init_doc();
        let mut s = doc.app_state().lock().unwrap();
        let bytes = s.store.encode();
        let mut new_store = decode_container_store(bytes);
        s.store.check_eq_after_parsing(&mut new_store);
    }
}
