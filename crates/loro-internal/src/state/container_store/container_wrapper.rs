use bytes::Bytes;
use loro_common::{
    ContainerID, ContainerType, InternalString, LoroMapValue, LoroResult, LoroValue,
};
use tracing::trace;

#[cfg(feature = "counter")]
use crate::state::counter_state::CounterState;
use crate::{
    arena::SharedArena,
    container::idx::ContainerIdx,
    state::{
        unknown_state::UnknownState, ContainerCreationContext, ContainerState, FastStateSnapshot,
        IndexType, ListState, MapState, MovableListState, RichtextState, State, TreeState,
    },
};

#[derive(Debug)]
pub(crate) struct ContainerWrapper {
    depth: usize,
    kind: ContainerType,
    parent: Option<ContainerID>,
    data: ContainerData,
    flushed: bool,
}

#[derive(Debug)]
enum ContainerData {
    State(State),
    Lazy(Box<LazyContainerData>),
}

#[derive(Debug)]
struct LazyContainerData {
    /// Lazily decoded snapshot bytes and optional decoded value.
    bytes: Option<Bytes>,
    value: Option<LoroValue>,
    bytes_offset_for_value: Option<usize>,
    bytes_offset_for_state: Option<usize>,
}

fn sorted_lazy_map_entry_refs(map: &LoroMapValue) -> Vec<(&String, &LoroValue)> {
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_unstable_by(|(left_key, _), (right_key, _)| left_key.cmp(right_key));
    entries
}

fn sorted_lazy_map_owned_entries(map: LoroMapValue) -> Vec<(InternalString, LoroValue)> {
    let mut entries: Vec<_> = map.unwrap().into_iter().collect();
    entries.sort_unstable_by(|(left_key, _), (right_key, _)| left_key.cmp(right_key));
    entries
        .into_iter()
        .map(|(key, value)| (key.into(), value))
        .collect()
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
            data: ContainerData::State(state),
            flushed: false,
        }
    }

    pub fn depth(&self) -> usize {
        self.depth
    }

    pub(crate) fn kind(&self) -> ContainerType {
        self.kind
    }

    /// It will not decode the state if it is not decoded
    #[allow(unused)]
    pub fn try_get_state(&self) -> Option<&State> {
        match &self.data {
            ContainerData::State(state) => Some(state),
            ContainerData::Lazy(_) => None,
        }
    }

    /// It will decode the state if it is not decoded
    pub fn get_state(&mut self, idx: ContainerIdx, ctx: ContainerCreationContext) -> &State {
        self.decode_state(idx, ctx).unwrap();
        match &self.data {
            ContainerData::State(state) => state,
            ContainerData::Lazy(_) => unreachable!("ContainerWrapper state should be decoded"),
        }
    }

    /// It will decode the state if it is not decoded
    pub fn get_state_mut(
        &mut self,
        idx: ContainerIdx,
        ctx: ContainerCreationContext,
    ) -> &mut State {
        self.decode_state(idx, ctx).unwrap();
        self.flushed = false;
        match &mut self.data {
            ContainerData::State(state) => state,
            ContainerData::Lazy(_) => unreachable!("ContainerWrapper state should be decoded"),
        }
    }

    pub fn get_value(&mut self, idx: ContainerIdx, ctx: ContainerCreationContext) -> LoroValue {
        match &mut self.data {
            ContainerData::State(state) => {
                trace!("state");
                state.get_value()
            }
            ContainerData::Lazy(lazy) if lazy.value.is_some() => {
                trace!("value");
                lazy.value.as_ref().unwrap().clone()
            }
            ContainerData::Lazy(_) => {
                trace!("transient value");
                self.decode_value_transient(idx, ctx).unwrap()
            }
        }
    }

    pub fn map_get(
        &mut self,
        idx: ContainerIdx,
        ctx: ContainerCreationContext,
        key: &str,
    ) -> Option<LoroValue> {
        match &mut self.data {
            ContainerData::State(state) => state.as_map_state().unwrap().get(key).cloned(),
            ContainerData::Lazy(_) => match self.get_value(idx, ctx) {
                LoroValue::Map(map) => map.get(key).cloned(),
                _ => None,
            },
        }
    }

    pub fn map_len(&mut self, idx: ContainerIdx, ctx: ContainerCreationContext) -> usize {
        match &mut self.data {
            ContainerData::State(state) => state.as_map_state().unwrap().len(),
            ContainerData::Lazy(_) => match self.get_value(idx, ctx) {
                LoroValue::Map(map) => map.len(),
                _ => 0,
            },
        }
    }

    pub fn map_keys(
        &mut self,
        idx: ContainerIdx,
        ctx: ContainerCreationContext,
    ) -> Vec<InternalString> {
        match &mut self.data {
            ContainerData::State(state) => state
                .as_map_state()
                .unwrap()
                .iter()
                .filter_map(|(key, value)| value.value.is_some().then(|| key.clone()))
                .collect(),
            ContainerData::Lazy(_) => match self.get_value(idx, ctx) {
                LoroValue::Map(map) => sorted_lazy_map_entry_refs(&map)
                    .into_iter()
                    .map(|(key, _)| key.as_str().into())
                    .collect(),
                _ => Vec::new(),
            },
        }
    }

    pub fn map_values(
        &mut self,
        idx: ContainerIdx,
        ctx: ContainerCreationContext,
    ) -> Vec<LoroValue> {
        match &mut self.data {
            ContainerData::State(state) => state
                .as_map_state()
                .unwrap()
                .iter()
                .filter_map(|(_, value)| value.value.clone())
                .collect(),
            ContainerData::Lazy(_) => match self.get_value(idx, ctx) {
                LoroValue::Map(map) => sorted_lazy_map_entry_refs(&map)
                    .into_iter()
                    .map(|(_, value)| value.clone())
                    .collect(),
                _ => Vec::new(),
            },
        }
    }

    pub fn map_entries(
        &mut self,
        idx: ContainerIdx,
        ctx: ContainerCreationContext,
    ) -> Vec<(InternalString, LoroValue)> {
        match &mut self.data {
            ContainerData::State(state) => state
                .as_map_state()
                .unwrap()
                .iter()
                .filter_map(|(key, value)| value.value.clone().map(|value| (key.clone(), value)))
                .collect(),
            ContainerData::Lazy(_) => match self.get_value(idx, ctx) {
                LoroValue::Map(map) => sorted_lazy_map_owned_entries(map),
                _ => Vec::new(),
            },
        }
    }

    pub fn list_get(
        &mut self,
        idx: ContainerIdx,
        ctx: ContainerCreationContext,
        index: usize,
    ) -> Option<LoroValue> {
        match &mut self.data {
            ContainerData::State(state) => match self.kind {
                ContainerType::List => state.as_list_state().unwrap().get(index).cloned(),
                ContainerType::MovableList => state
                    .as_movable_list_state()
                    .unwrap()
                    .get(index, IndexType::ForUser)
                    .cloned(),
                _ => None,
            },
            ContainerData::Lazy(_) => match self.get_value(idx, ctx) {
                LoroValue::List(list) => list.get(index).cloned(),
                _ => None,
            },
        }
    }

    pub fn list_len(&mut self, idx: ContainerIdx, ctx: ContainerCreationContext) -> usize {
        match &mut self.data {
            ContainerData::State(state) => match self.kind {
                ContainerType::List => state.as_list_state().unwrap().len(),
                ContainerType::MovableList => state.as_movable_list_state().unwrap().len(),
                _ => 0,
            },
            ContainerData::Lazy(_) => match self.get_value(idx, ctx) {
                LoroValue::List(list) => list.len(),
                _ => 0,
            },
        }
    }

    pub fn list_values(
        &mut self,
        idx: ContainerIdx,
        ctx: ContainerCreationContext,
    ) -> Vec<LoroValue> {
        match &mut self.data {
            ContainerData::State(state) => match self.kind {
                ContainerType::List => state.as_list_state().unwrap().iter().cloned().collect(),
                ContainerType::MovableList => state
                    .as_movable_list_state()
                    .unwrap()
                    .iter()
                    .cloned()
                    .collect(),
                _ => Vec::new(),
            },
            ContainerData::Lazy(_) => match self.get_value(idx, ctx) {
                LoroValue::List(list) => list.iter().cloned().collect(),
                _ => Vec::new(),
            },
        }
    }

    pub fn encode(&mut self) -> Bytes {
        let ContainerData::State(state) = &mut self.data else {
            let lazy = match &self.data {
                ContainerData::Lazy(lazy) => lazy,
                ContainerData::State(_) => unreachable!(),
            };
            assert!(self.flushed, "lazy container should be flushed");
            return lazy.bytes.as_ref().unwrap().clone();
        };

        // ContainerType
        // Depth
        // ParentID
        // StateSnapshot
        let mut output = Vec::new();
        output.push(self.kind.to_u8());
        leb128::write::unsigned(&mut output, self.depth as u64).unwrap();
        postcard::to_io(&self.parent, &mut output).unwrap();
        state.encode_snapshot_fast(&mut output);
        output.into()
    }

    #[allow(unused)]
    pub fn decode_parent(b: &[u8]) -> Option<ContainerID> {
        let mut bytes = &b[1..];
        let _depth = leb128::read::unsigned(&mut bytes).unwrap();
        let (parent, _bytes) = postcard::take_from_bytes::<Option<ContainerID>>(bytes).unwrap();
        parent
    }

    pub fn new_from_bytes(bytes: Bytes) -> Self {
        let kind = ContainerType::try_from_u8(bytes[0]).unwrap();
        let mut reader = &bytes[1..];
        let depth = leb128::read::unsigned(&mut reader).unwrap();
        let (parent, reader) = postcard::take_from_bytes(reader).unwrap();
        let size = bytes.len() - reader.len();
        Self {
            depth: depth as usize,
            kind,
            parent,
            data: ContainerData::Lazy(Box::new(LazyContainerData {
                value: None,
                bytes: Some(bytes.clone()),
                bytes_offset_for_value: Some(size),
                bytes_offset_for_state: None,
            })),
            flushed: true,
        }
    }

    fn decode_value(&mut self, idx: ContainerIdx, ctx: ContainerCreationContext) -> LoroResult<()> {
        if matches!(self.data, ContainerData::State(_)) {
            return Ok(());
        }

        if matches!(&self.data, ContainerData::Lazy(lazy) if lazy.value.is_some()) {
            return Ok(());
        }

        let (v, state_offset, decoded_state) = self.decode_value_from_bytes(idx, ctx)?;
        if let Some(state) = decoded_state {
            self.data = ContainerData::State(state);
            return Ok(());
        }

        let ContainerData::Lazy(lazy) = &mut self.data else {
            unreachable!();
        };
        lazy.value = Some(v);
        lazy.bytes_offset_for_state = Some(state_offset);
        Ok(())
    }

    fn decode_value_transient(
        &mut self,
        idx: ContainerIdx,
        ctx: ContainerCreationContext,
    ) -> LoroResult<LoroValue> {
        let (value, state_offset, _) = self.decode_value_from_bytes(idx, ctx)?;
        if let ContainerData::Lazy(lazy) = &mut self.data {
            lazy.bytes_offset_for_state = Some(state_offset);
        }
        Ok(value)
    }

    fn value_bytes_and_offset(&mut self) -> Option<(Bytes, usize)> {
        let ContainerData::Lazy(lazy) = &mut self.data else {
            return None;
        };

        let bytes = lazy.bytes.as_ref()?.clone();
        if lazy.bytes_offset_for_value.is_none() {
            let mut reader: &[u8] = &bytes;
            reader = &reader[1..];
            let _depth = leb128::read::unsigned(&mut reader).unwrap();
            let (_parent, reader) =
                postcard::take_from_bytes::<Option<ContainerID>>(reader).unwrap();
            // SAFETY: bytes is a slice of b
            let size = bytes.len() - reader.len();
            lazy.bytes_offset_for_value = Some(size);
        }

        Some((bytes, lazy.bytes_offset_for_value.unwrap()))
    }

    fn decode_value_from_bytes(
        &mut self,
        idx: ContainerIdx,
        ctx: ContainerCreationContext,
    ) -> LoroResult<(LoroValue, usize, Option<State>)> {
        let Some((bytes, value_offset)) = self.value_bytes_and_offset() else {
            return Ok((self.kind.default_value(), 0, None));
        };
        let b = &bytes[value_offset..];

        let mut decoded_state = None;
        let (v, state_offset) = match self.kind {
            ContainerType::Text => {
                let (v, rest) = RichtextState::decode_value(b)?;
                (v, b.len() - rest.len() + value_offset)
            }
            ContainerType::Map => {
                let (v, rest) = MapState::decode_value(b)?;
                (v, b.len() - rest.len() + value_offset)
            }
            ContainerType::List => {
                let (v, rest) = ListState::decode_value(b)?;
                (v, b.len() - rest.len() + value_offset)
            }
            ContainerType::MovableList => {
                let (v, rest) = MovableListState::decode_value(b)?;
                (v, b.len() - rest.len() + value_offset)
            }
            ContainerType::Tree => {
                let mut state = TreeState::decode_snapshot_fast(idx, (LoroValue::Null, b), ctx)?;
                let value = state.get_value();
                decoded_state = Some(State::TreeState(Box::new(state)));
                (value, value_offset)
            }
            #[cfg(feature = "counter")]
            ContainerType::Counter => {
                let (v, _rest) = CounterState::decode_value(b)?;
                (v, 0)
            }
            ContainerType::Unknown(_) => {
                let (v, rest) = UnknownState::decode_value(b)?;
                (v, b.len() - rest.len() + value_offset)
            }
        };

        Ok((v, state_offset, decoded_state))
    }

    pub(super) fn decode_state(
        &mut self,
        idx: ContainerIdx,
        ctx: ContainerCreationContext,
    ) -> LoroResult<()> {
        if matches!(self.data, ContainerData::State(_)) {
            return Ok(());
        }

        let need_value = match &self.data {
            ContainerData::Lazy(lazy) => lazy.value.is_none(),
            ContainerData::State(_) => false,
        };
        if need_value {
            self.decode_value(idx, ctx)?;
        }

        if matches!(self.data, ContainerData::State(_)) {
            return Ok(());
        }

        let ContainerData::Lazy(lazy) = &self.data else {
            unreachable!();
        };
        let bytes = lazy.bytes.as_ref().unwrap();
        let offset = lazy.bytes_offset_for_state.unwrap();
        let b = &bytes[offset..];
        let v = lazy.value.as_ref().unwrap().clone();
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
        self.data = ContainerData::State(state);
        Ok(())
    }

    #[allow(unused)]
    pub(crate) fn is_state_empty(&self) -> bool {
        match &self.data {
            ContainerData::State(state) => state.is_state_empty(),
            ContainerData::Lazy(lazy) => {
                // FIXME: it's not very accurate...
                lazy.bytes.as_ref().unwrap().len() > 10
            }
        }
    }

    pub(crate) fn clear_bytes(&mut self) {
        assert!(matches!(self.data, ContainerData::State(_)));
    }

    pub(crate) fn is_flushed(&self) -> bool {
        self.flushed
    }

    pub(crate) fn set_flushed(&mut self, flushed: bool) {
        self.flushed = flushed;
    }

    #[allow(unused)]
    pub(crate) fn parent(&self) -> Option<&ContainerID> {
        self.parent.as_ref()
    }
}
