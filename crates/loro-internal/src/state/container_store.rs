use super::{ContainerCreationContext, State};
use crate::{
    arena::SharedArena,
    configure::Configure,
    container::idx::ContainerIdx,
    utils::kv_wrapper::KvWrapper,
    version::Frontiers,
};
use bytes::Bytes;
use inner_store::InnerStore;
use loro_common::{LoroResult, LoroValue};
use std::sync::{atomic::AtomicU64, Arc, Mutex};

pub(crate) use container_wrapper::ContainerWrapper;

mod container_wrapper;
mod inner_store;

///  Encoding Schema for Container Store
///
/// ┌───────────────┬───────────────────────────────────┐
/// │ 4B Container  │          N CID + Offsets          │
/// │ Binary Offset │       (EncodedBy DeltaRLE)        │
/// └───────────────┴───────────────────────────────────┘
/// ┌───────────────────────────────────────────────────┐
/// │                                                   │
/// │                                                   │
/// │                                                   │
/// │              All Containers' Binary               │
/// │                                                   │
/// │                                                   │
/// │                                                   │
/// └───────────────────────────────────────────────────┘
///
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
    gc_store: Option<Arc<GcStore>>,
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
    pub trimmed_frontiers: Frontiers,
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
            store: InnerStore::new(arena.clone()),
            arena,
            conf,
            gc_store: None,
            peer,
        }
    }

    pub fn can_import_snapshot(&self) -> bool {
        if self.gc_store.is_some() {
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

    pub fn gc_store(&self) -> Option<&Arc<GcStore>> {
        self.gc_store.as_ref()
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

    pub fn encode_gc(&mut self) -> Bytes {
        if let Some(gc) = self.gc_store.as_mut() {
            gc.store.try_lock().unwrap().get_kv().export()
        } else {
            Bytes::new()
        }
    }

    pub fn trimmed_frontiers(&self) -> Option<&Frontiers> {
        self.gc_store.as_ref().map(|x| &x.trimmed_frontiers)
    }

    pub(crate) fn decode(&mut self, bytes: Bytes) -> LoroResult<Option<Frontiers>> {
        self.store.decode(bytes)
    }

    pub(crate) fn decode_gc(
        &mut self,
        gc_bytes: Bytes,
        start_frontiers: Frontiers,
    ) -> LoroResult<Option<Frontiers>> {
        assert!(self.gc_store.is_none());
        let mut inner = InnerStore::new(self.arena.clone());
        let f = inner.decode(gc_bytes)?;
        self.gc_store = Some(Arc::new(GcStore {
            trimmed_frontiers: start_frontiers,
            store: Mutex::new(inner),
        }));
        Ok(f)
    }

    pub(crate) fn decode_state_by_two_bytes(
        &mut self,
        gc_bytes: Bytes,
        state_bytes: Bytes,
    ) -> LoroResult<()> {
        self.store.decode_twice(gc_bytes.clone(), state_bytes)?;
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

    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }

    pub fn len(&self) -> usize {
        self.store.len()
    }

    pub fn iter_all_containers(
        &mut self,
    ) -> impl Iterator<Item = (&ContainerIdx, &mut ContainerWrapper)> {
        self.store.iter_all_containers_mut()
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
        &self,
        arena: SharedArena,
        peer: Arc<AtomicU64>,
        config: Configure,
    ) -> ContainerStore {
        ContainerStore {
            store: self.store.fork(arena.clone()),
            arena,
            conf: config,
            peer,
            gc_store: None,
        }
    }

    #[allow(unused)]
    fn check_eq_after_parsing(&mut self, other: &mut ContainerStore) {
        if self.store.len() != other.store.len() {
            panic!("store len mismatch");
        }

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
    use loro_common::{ContainerID, ContainerType, Counter, InternalString, LoroError, LoroResult};
    use serde::{Deserialize, Serialize};
    use serde_columnar::{
        AnyRleDecoder, AnyRleEncoder, BoolRleDecoder, BoolRleEncoder, DeltaRleDecoder,
        DeltaRleEncoder,
    };
    use std::{borrow::Cow, io::Write};

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
    #[derive(Default)]
    pub(super) struct CidOffsetEncoder {
        types: AnyRleEncoder<u8>,
        is_root_bools: BoolRleEncoder,
        strings: Vec<InternalString>,
        peer_ids: AnyRleEncoder<u64>,
        counters: DeltaRleEncoder,
        offsets: DeltaRleEncoder,
    }

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

    impl CidOffsetEncoder {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn push(&mut self, cid: &ContainerID, offset: usize) {
            self.types.append(cid.container_type().to_u8()).unwrap();
            self.is_root_bools.append(cid.is_root()).unwrap();
            match cid {
                ContainerID::Root { name, .. } => {
                    self.strings.push(name.clone());
                }
                ContainerID::Normal { peer, counter, .. } => {
                    self.peer_ids.append(*peer).unwrap();
                    self.counters.append(*counter).unwrap();
                }
            }

            self.offsets.append(offset).unwrap();
        }

        pub fn write_to_io<W: Write>(self, w: W) {
            let mut strings = Vec::with_capacity(self.strings.iter().map(|s| s.len() + 4).sum());
            for s in self.strings {
                leb128::write::unsigned(&mut strings, s.len() as u64).unwrap();
                strings.extend(s.as_bytes());
            }

            let t = EncodedCid {
                types: self.types.finish().unwrap().into(),
                is_root_bools: self.is_root_bools.finish().unwrap().into(),
                strings: strings.into(),
                peer_ids: self.peer_ids.finish().unwrap().into(),
                counters: self.counters.finish().unwrap().into(),
                offsets: self.offsets.finish().unwrap().into(),
            };

            postcard::to_io(&t, w).unwrap();
        }
    }

    pub(super) fn decode_cids(bytes: &[u8]) -> LoroResult<Vec<(ContainerID, usize)>> {
        let EncodedCid {
            types,
            is_root_bools,
            strings: strings_bytes,
            peer_ids: peers_bytes,
            counters,
            offsets,
        } = postcard::from_bytes(bytes).map_err(|e| {
            LoroError::DecodeError(format!("Decode cids error {}", e).into_boxed_str())
        })?;

        let mut ans = Vec::new();
        let types: AnyRleDecoder<u8> = AnyRleDecoder::new(&types);
        let is_root_iter = BoolRleDecoder::new(&is_root_bools);
        let mut strings = Vec::new();
        {
            // decode strings
            let mut strings_bytes: &[u8] = &strings_bytes;
            while !strings_bytes.is_empty() {
                let len = leb128::read::unsigned(&mut strings_bytes).unwrap();
                let s = std::str::from_utf8(&strings_bytes[..len as usize]).unwrap();
                strings.push(InternalString::from(s));
                strings_bytes = &strings_bytes[len as usize..];
            }
        }

        let mut counters: DeltaRleDecoder<i32> = DeltaRleDecoder::new(&counters);
        let mut offsets: DeltaRleDecoder<usize> = DeltaRleDecoder::new(&offsets);
        let mut peer_iter: AnyRleDecoder<u64> = AnyRleDecoder::new(&peers_bytes);
        let mut s_iter = strings.into_iter();

        for (t, is_root) in types.zip(is_root_iter) {
            let ty = ContainerType::try_from_u8(t.unwrap()).unwrap();
            let offset = offsets.next().unwrap().unwrap();
            match is_root.unwrap() {
                true => {
                    let s = s_iter.next();
                    ans.push((
                        ContainerID::Root {
                            name: s.unwrap(),
                            container_type: ty,
                        },
                        offset,
                    ))
                }
                false => ans.push((
                    ContainerID::Normal {
                        peer: peer_iter.next().unwrap().unwrap(),
                        counter: counters.next().unwrap().unwrap() as Counter,
                        container_type: ty,
                    },
                    offset,
                )),
            }
        }

        Ok(ans)
    }

    #[cfg(test)]
    mod test {
        use super::*;

        #[test]
        fn test_container_store() {
            let mut encoder = CidOffsetEncoder::new();
            let input = [
                (
                    ContainerID::Root {
                        name: "map".into(),
                        container_type: ContainerType::Map,
                    },
                    0,
                ),
                (
                    ContainerID::Root {
                        name: "list".into(),
                        container_type: ContainerType::List,
                    },
                    1,
                ),
                (
                    ContainerID::Normal {
                        peer: 1,
                        counter: 0,
                        container_type: ContainerType::Map,
                    },
                    2,
                ),
                (
                    ContainerID::Normal {
                        peer: 2,
                        counter: 1,
                        container_type: ContainerType::List,
                    },
                    4,
                ),
            ];
            for (cid, offset) in input.iter() {
                encoder.push(cid, *offset);
            }
            let mut bytes = Vec::new();
            encoder.write_to_io(&mut bytes);

            let cids = decode_cids(&bytes).unwrap();
            assert_eq!(&cids, &input)
        }
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
