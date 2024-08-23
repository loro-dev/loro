use bytes::Bytes;
use loro_common::{ContainerID, ContainerType, LoroResult, LoroValue};

#[cfg(feature = "counter")]
use crate::state::counter_state::CounterState;
use crate::{
    arena::SharedArena,
    container::idx::ContainerIdx,
    state::{
        unknown_state::UnknownState, ContainerCreationContext, ContainerState, FastStateSnapshot,
        ListState, MapState, MovableListState, RichtextState, State, TreeState,
    },
};

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
    flushed: bool,
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
            flushed: false,
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
        self.flushed = false;
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
            state: None,
            value: None,
            bytes: Some(bytes.clone()),
            bytes_offset_for_value: Some(size),
            bytes_offset_for_state: None,
            flushed: true,
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
        if self.value.is_some() || self.state.is_some() {
            return Ok(());
        }

        let Some(bytes) = self.bytes.as_ref() else {
            return Ok(());
        };

        if self.bytes_offset_for_value.is_none() {
            let mut reader: &[u8] = bytes;
            reader = &reader[1..];
            let _depth = leb128::read::unsigned(&mut reader).unwrap();
            let (_parent, reader) =
                postcard::take_from_bytes::<Option<ContainerID>>(reader).unwrap();
            // SAFETY: bytes is a slice of b
            let size = bytes.len() - reader.len();
            self.bytes_offset_for_value = Some(size);
        }

        let value_offset = self.bytes_offset_for_value.unwrap();
        let b = &bytes[value_offset..];

        let (v, rest) = match self.kind {
            ContainerType::Text => RichtextState::decode_value(b)?,
            ContainerType::Map => MapState::decode_value(b)?,
            ContainerType::List => ListState::decode_value(b)?,
            ContainerType::MovableList => MovableListState::decode_value(b)?,
            ContainerType::Tree => {
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

    pub(super) fn decode_state(
        &mut self,
        idx: ContainerIdx,
        ctx: ContainerCreationContext,
    ) -> LoroResult<()> {
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

    pub(crate) fn clear_bytes(&mut self) {
        assert!(self.state.is_some());
        self.bytes = None;
        self.bytes_offset_for_state = None;
        self.bytes_offset_for_value = None;
    }

    pub(crate) fn is_flushed(&self) -> bool {
        self.flushed
    }

    pub(crate) fn parent(&self) -> Option<&ContainerID> {
        self.parent.as_ref()
    }
}
