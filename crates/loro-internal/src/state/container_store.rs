use std::{
    cmp::Ordering,
    sync::{atomic::AtomicU64, Arc},
};

use crate::{
    arena::SharedArena,
    configure::Configure,
    container::idx::ContainerIdx,
    state::{FastStateSnapshot, RichtextState},
};
use bytes::Bytes;
use encode::{decode_cids, CidOffsetEncoder};
use fxhash::FxHashMap;
use loro_common::{ContainerID, ContainerType, LoroResult, LoroValue};

use super::{
    unknown_state::UnknownState, ContainerCreationContext, ContainerState, ListState, MapState,
    MovableListState, State, TreeState,
};

#[cfg(feature = "counter")]
use super::counter_state::CounterState;

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
#[derive(Clone)]
pub(crate) struct ContainerStore {
    arena: SharedArena,
    store: FxHashMap<ContainerIdx, ContainerWrapper>,
    conf: Configure,
    peer: Arc<AtomicU64>,
}

impl std::fmt::Debug for ContainerStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContainerStore")
            .field("store", &self.store)
            .finish()
    }
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
            arena,
            store: Default::default(),
            conf,
            peer,
        }
    }

    pub fn get_container_mut(&mut self, idx: ContainerIdx) -> Option<&mut State> {
        self.store.get_mut(&idx).map(|x| {
            x.get_state_mut(
                idx,
                ContainerCreationContext {
                    configure: &self.conf,
                    peer: self.peer.load(std::sync::atomic::Ordering::Relaxed),
                },
            )
        })
    }

    #[allow(unused)]
    pub fn get_container(&mut self, idx: ContainerIdx) -> Option<&State> {
        self.store.get_mut(&idx).map(|x| {
            x.get_state(
                idx,
                ContainerCreationContext {
                    configure: &self.conf,
                    peer: self.peer.load(std::sync::atomic::Ordering::Relaxed),
                },
            )
        })
    }

    pub fn get_value(&mut self, idx: ContainerIdx) -> Option<LoroValue> {
        self.store.get_mut(&idx).map(|c| {
            c.get_value(
                idx,
                ContainerCreationContext {
                    configure: &self.conf,
                    peer: self.peer.load(std::sync::atomic::Ordering::Relaxed),
                },
            )
        })
    }

    pub fn encode(&mut self) -> Bytes {
        let mut id_bytes_pairs = Vec::with_capacity(self.store.len());
        for (idx, container) in self.store.iter_mut() {
            let id = self.arena.get_container_id(*idx).unwrap();
            let encoded = container.encode();
            #[cfg(debug_assertions)]
            {
                let mut new = ContainerWrapper::new_from_bytes(encoded.clone());
                let value = new.get_value(*idx, ctx!(self));
                assert_eq!(value, container.get_value(*idx, ctx!(self)));
            }

            id_bytes_pairs.push((id, encoded))
        }

        id_bytes_pairs.sort_by(|(a, _), (b, _)| a.cmp(b));
        let mut id_encoder = CidOffsetEncoder::new();
        let mut offset = 0;
        for (id, bytes) in id_bytes_pairs.iter() {
            id_encoder.push(id, offset);
            offset += bytes.len();
        }

        let mut bytes = Vec::with_capacity(self.store.len() * 4 + 4);
        // prepend 4 zeros
        bytes.resize(4, 0);
        // append the encoded cids
        id_encoder.write_to_io(&mut bytes);
        // set the first 4 bytes of bytes to the length of itself
        let len = bytes.len() as u32;
        bytes[0] = (len & 0xff) as u8;
        bytes[1] = ((len >> 8) & 0xff) as u8;
        bytes[2] = ((len >> 16) & 0xff) as u8;
        bytes[3] = ((len >> 24) & 0xff) as u8;
        for (_id, b) in id_bytes_pairs.iter() {
            bytes.extend_from_slice(b);
        }

        bytes.into()
    }

    pub fn decode(&mut self, bytes: Bytes) -> LoroResult<()> {
        assert!(self.is_empty());
        let offset = u32::from_le_bytes((&bytes[..4]).try_into().unwrap()) as usize;
        let cids = &bytes[4..offset];
        let cids = decode_cids(cids)?;
        let container_bytes = bytes.slice(offset..);
        for (i, (cid, offset_start)) in cids.iter().enumerate() {
            let offset_end = if i < cids.len() - 1 {
                cids[i + 1].1
            } else {
                container_bytes.len()
            };

            let container =
                ContainerWrapper::new_from_bytes(container_bytes.slice(*offset_start..offset_end));
            let idx = self.arena.register_container(cid);
            let p = container
                .parent
                .as_ref()
                .map(|p| self.arena.register_container(p));
            self.arena.set_parent(idx, p);
            self.store.insert(idx, container);
        }

        Ok(())
    }

    pub fn iter_and_decode_all(&mut self) -> impl Iterator<Item = &mut State> {
        self.store.iter_mut().map(|(idx, v)| {
            v.get_state_mut(
                *idx,
                ContainerCreationContext {
                    configure: &self.conf,
                    peer: self.peer.load(std::sync::atomic::Ordering::Relaxed),
                },
            )
        })
    }

    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }

    pub fn len(&self) -> usize {
        self.store.len()
    }

    #[allow(unused)]
    pub fn iter(&self) -> impl Iterator<Item = (&ContainerIdx, &ContainerWrapper)> {
        self.store.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&ContainerIdx, &mut ContainerWrapper)> {
        self.store.iter_mut()
    }

    pub(super) fn get_or_create_mut(&mut self, idx: ContainerIdx) -> &mut State {
        self.store
            .entry(idx)
            .or_insert_with(|| {
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
            .entry(idx)
            .or_insert_with(|| {
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
        self.store.len() * 4
            + self
                .store
                .values()
                .map(|v| v.estimate_size())
                .sum::<usize>()
    }

    pub(super) fn contains(&self, idx: ContainerIdx) -> bool {
        self.store.contains_key(&idx)
    }

    pub(crate) fn fork(
        &self,
        arena: SharedArena,
        peer: Arc<AtomicU64>,
        config: Configure,
    ) -> ContainerStore {
        let mut store = FxHashMap::default();
        for (idx, container) in self.store.iter() {
            store.insert(*idx, container.clone());
        }

        ContainerStore {
            arena,
            store,
            conf: config,
            peer,
        }
    }

    #[allow(unused)]
    fn check_eq_after_parsing(&mut self, other: &mut ContainerStore) {
        if self.store.len() != other.store.len() {
            panic!("store len mismatch");
        }

        for (idx, container) in self.store.iter_mut() {
            let id = self.arena.get_container_id(*idx).unwrap();
            let other_idx = other.arena.register_container(&id);
            let other_container = other
                .store
                .get_mut(&other_idx)
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
            other_container.bytes = None;
            if container.encode() != other_container.encode() {
                panic!(
                    "container mismatch Origin: {:#?}, New: {:#?}",
                    &container, &other_container
                );
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ContainerWrapper {
    depth: usize,
    kind: ContainerType,
    parent: Option<ContainerID>,
    /// The possible combinations of is_some() are:
    ///
    /// 1. bytes: new container decoded from bytes
    /// 2. bytes + value: new container decoded from bytes, with value decoded
    /// 3. state + bytes + value: new container decoded from bytes, with value and state decoded
    /// 4. state
    bytes: Option<Bytes>,
    value: Option<LoroValue>,
    bytes_offset_for_value: Option<usize>,
    bytes_offset_for_state: Option<usize>,
    state: Option<State>,
}

impl ContainerWrapper {
    pub fn new(state: State, arena: &SharedArena) -> Self {
        let idx = state.container_idx();
        let parent = arena
            .get_parent(idx)
            .and_then(|p| arena.get_container_id(p));
        let depth = arena.get_depth(idx).unwrap().get() as usize;
        Self {
            depth,
            parent,
            kind: idx.get_type(),
            state: Some(state),
            bytes: None,
            value: None,
            bytes_offset_for_state: None,
            bytes_offset_for_value: None,
        }
    }

    pub fn depth(&self) -> usize {
        self.depth
    }

    /// It will not decode the state if it is not decoded
    #[allow(unused)]
    pub fn try_get_state(&self) -> Option<&State> {
        self.state.as_ref()
    }

    /// It will decode the state if it is not decoded
    pub fn get_state(&mut self, idx: ContainerIdx, ctx: ContainerCreationContext) -> &State {
        self.decode_state(idx, ctx).unwrap();
        self.state.as_ref().expect("ContainerWrapper is empty")
    }

    /// It will decode the state if it is not decoded
    pub fn get_state_mut(
        &mut self,
        idx: ContainerIdx,
        ctx: ContainerCreationContext,
    ) -> &mut State {
        self.decode_state(idx, ctx).unwrap();
        self.bytes = None;
        self.value = None;
        self.state.as_mut().unwrap()
    }

    pub fn get_value(&mut self, idx: ContainerIdx, ctx: ContainerCreationContext) -> LoroValue {
        if let Some(v) = self.value.as_ref() {
            return v.clone();
        }

        self.decode_value(idx, ctx).unwrap();
        if self.value.is_none() {
            return self.state.as_mut().unwrap().get_value();
        }

        self.value.as_ref().unwrap().clone()
    }

    pub fn encode(&mut self) -> Bytes {
        if let Some(bytes) = self.bytes.as_ref() {
            return bytes.clone();
        }

        // ContainerType
        // Depth
        // ParentID
        // StateSnapshot
        let mut output = Vec::new();
        output.push(self.kind.to_u8());
        leb128::write::unsigned(&mut output, self.depth as u64).unwrap();
        postcard::to_io(&self.parent, &mut output).unwrap();
        self.state
            .as_mut()
            .unwrap()
            .encode_snapshot_fast(&mut output);
        let ans: Bytes = output.into();
        self.bytes = Some(ans.clone());
        ans
    }

    pub fn new_from_bytes(b: Bytes) -> Self {
        let src: &[u8] = &b;
        let bytes: &[u8] = &b;
        let kind = ContainerType::try_from_u8(bytes[0]).unwrap();
        let mut bytes = &bytes[1..];
        let depth = leb128::read::unsigned(&mut bytes).unwrap();
        let (parent, bytes) = postcard::take_from_bytes(bytes).unwrap();
        // SAFETY: bytes is a slice of b
        let size = unsafe { bytes.as_ptr().offset_from(src.as_ptr()) };
        Self {
            depth: depth as usize,
            kind,
            parent,
            state: None,
            value: None,
            bytes: Some(b.clone()),
            bytes_offset_for_value: Some(size as usize),
            bytes_offset_for_state: None,
        }
    }

    #[allow(unused)]
    pub fn ensure_value(&mut self, idx: ContainerIdx, ctx: ContainerCreationContext) -> &LoroValue {
        if self.value.is_some() {
        } else if self.state.is_some() {
            let value = self.state.as_mut().unwrap().get_value();
            self.value = Some(value);
        } else {
            self.decode_value(idx, ctx).unwrap();
        }

        self.value.as_ref().unwrap()
    }

    fn decode_value(&mut self, idx: ContainerIdx, ctx: ContainerCreationContext) -> LoroResult<()> {
        let Some(b) = self.bytes.as_ref() else {
            return Ok(());
        };

        let value_offset = self.bytes_offset_for_value.unwrap();
        let b = &b[value_offset..];

        let (v, rest) = match self.kind {
            ContainerType::Text => RichtextState::decode_value(b)?,
            ContainerType::Map => MapState::decode_value(b)?,
            ContainerType::List => ListState::decode_value(b)?,
            ContainerType::MovableList => MovableListState::decode_value(b)?,
            ContainerType::Tree => {
                assert!(self.state.is_none());
                let mut state = TreeState::decode_snapshot_fast(idx, (LoroValue::Null, b), ctx)?;
                self.value = Some(state.get_value());
                self.state = Some(State::TreeState(Box::new(state)));
                self.bytes_offset_for_state = Some(value_offset);
                return Ok(());
            }
            #[cfg(feature = "counter")]
            ContainerType::Counter => {
                let (v, _rest) = CounterState::decode_value(b)?;
                self.value = Some(v);
                self.bytes_offset_for_state = Some(0);
                return Ok(());
            }
            ContainerType::Unknown(_) => UnknownState::decode_value(b)?,
        };

        self.value = Some(v);
        // SAFETY: rest is a slice of b
        let offset = unsafe { rest.as_ptr().offset_from(b.as_ptr()) };
        self.bytes_offset_for_state = Some(offset as usize + value_offset);
        Ok(())
    }

    fn decode_state(&mut self, idx: ContainerIdx, ctx: ContainerCreationContext) -> LoroResult<()> {
        if self.state.is_some() {
            return Ok(());
        }

        if self.value.is_none() {
            self.decode_value(idx, ctx)?;
        }

        let b = self.bytes.as_ref().unwrap();
        let offset = self.bytes_offset_for_state.unwrap();
        let b = &b[offset..];
        let v = self.value.as_ref().unwrap().clone();
        let state: State = match self.kind {
            ContainerType::Text => RichtextState::decode_snapshot_fast(idx, (v, b), ctx)?.into(),
            ContainerType::Map => MapState::decode_snapshot_fast(idx, (v, b), ctx)?.into(),
            ContainerType::List => ListState::decode_snapshot_fast(idx, (v, b), ctx)?.into(),
            ContainerType::MovableList => {
                MovableListState::decode_snapshot_fast(idx, (v, b), ctx)?.into()
            }
            ContainerType::Tree => TreeState::decode_snapshot_fast(idx, (v, b), ctx)?.into(),
            #[cfg(feature = "counter")]
            ContainerType::Counter => CounterState::decode_snapshot_fast(idx, (v, b), ctx)?.into(),
            ContainerType::Unknown(_) => {
                UnknownState::decode_snapshot_fast(idx, (v, b), ctx)?.into()
            }
        };
        self.state = Some(state);
        Ok(())
    }

    pub fn estimate_size(&self) -> usize {
        if let Some(bytes) = self.bytes.as_ref() {
            return bytes.len();
        }

        self.state.as_ref().unwrap().estimate_size()
    }

    #[allow(unused)]
    pub(crate) fn is_state_empty(&self) -> bool {
        if let Some(state) = self.state.as_ref() {
            return state.is_state_empty();
        }

        // FIXME: it's not very accurate...
        self.bytes.as_ref().unwrap().len() > 10
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
    use crate::{ListHandler, LoroDoc, MapHandler, MovableListHandler};

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
        let root = tree.create(None).unwrap();
        tree.create_at(Some(root), 0).unwrap();

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
