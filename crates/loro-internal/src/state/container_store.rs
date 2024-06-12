use crate::{arena::SharedArena, container::idx::ContainerIdx};
use bytes::Bytes;
use fxhash::FxHashMap;
use loro_common::{ContainerID, LoroValue};
use once_cell::sync::OnceCell;

use super::{ContainerState, State};

/// ```log
///
///      Encoding Schema for Container Store
///
///     ┌───────────────────────────────────────────────────┐
///     │                  N CID + Offsets                  │
///     │               (EncodedBy DeltaRLE)                │
///     └───────────────────────────────────────────────────┘
///     ┌───────────────────────────────────────────────────┐
///     │                                                   │
///     │                                                   │
///     │                                                   │
///     │              All Containers' Binary               │
///     │                                                   │
///     │                                                   │
///     │                                                   │
///     └───────────────────────────────────────────────────┘
///
///
///     ─ ─ ─ ─ ─ ─ ─ For Each Container Type ─ ─ ─ ─ ─ ─ ─ ─
///
///     ┌────────────────┬──────────────────────────────────┐
///     │   u16 Depth    │             ParentID             │
///     └────────────────┴──────────────────────────────────┘
///     ┌───────────────────────────────────────────────────┐
///     │ ┌───────────────────────────────────────────────┐ │
///     │ │                Into<LoroValue>                │ │
///     │ └───────────────────────────────────────────────┘ │
///     │                                                   │
///     │             Container Specific Encode             │
///     │                                                   │
///     │                                                   │
///     │                                                   │
///     │                                                   │
///     └───────────────────────────────────────────────────┘
/// ```
pub(crate) struct ContainerStore {
    arena: SharedArena,
    store: FxHashMap<ContainerIdx, ContainerWrapper>,
}

impl ContainerStore {
    pub fn get_container(&mut self, idx: ContainerIdx) -> Option<&mut ContainerWrapper> {
        self.store.get_mut(&idx)
    }

    pub fn get_value(&mut self, idx: ContainerIdx) -> Option<LoroValue> {
        self.store.get_mut(&idx).and_then(|c| c.get_value())
    }

    pub fn from_bytes(bytes: Bytes) -> Self {
        todo!("decode all containers into bytes")
    }

    pub fn encode(&self) -> Bytes {
        todo!("encode all containers into bytes")
    }
}

pub(crate) enum ContainerWrapper {
    Bytes(Bytes),
    PartialParsed { bytes: Bytes, value: LoroValue },
    Parsed { bytes: Bytes, state: State },
    State(State),
}

impl ContainerWrapper {
    pub fn get_state(&mut self) -> Option<&State> {
        match self {
            ContainerWrapper::Bytes(_) => todo!(),
            ContainerWrapper::PartialParsed { bytes, value } => todo!(),
            ContainerWrapper::Parsed { bytes, state } => todo!(),
            ContainerWrapper::State(_) => todo!(),
        }
    }

    pub fn get_value(&mut self) -> Option<LoroValue> {
        match self {
            ContainerWrapper::Bytes(bytes) => todo!("partial parse"),
            ContainerWrapper::PartialParsed { bytes, value } => Some(value.clone()),
            ContainerWrapper::Parsed { bytes, state } => Some(state.get_value()),
            ContainerWrapper::State(s) => Some(s.get_value()),
        }
    }
}

mod encode {
    use loro_common::ContainerID;
    use serde::{Deserialize, Serialize};
    use std::borrow::Cow;

    #[derive(Serialize, Deserialize)]
    struct EncodedStateStore<'a> {
        #[serde(borrow)]
        cids: Cow<'a, [u8]>,
        #[serde(borrow)]
        bytes: Cow<'a, [u8]>,
    }

    pub(super) struct CidOffsetEncoder {}

    impl CidOffsetEncoder {
        pub fn push(&mut self, cid: &ContainerID, offset: usize) {
            todo!()
        }

        pub fn finish(self) -> Vec<u8> {
            todo!()
        }
    }

    pub(super) struct CidOffsetDecoder {}

    impl Iterator for CidOffsetDecoder {
        type Item = (ContainerID, usize);

        fn next(&mut self) -> Option<Self::Item> {
            todo!()
        }
    }
}
