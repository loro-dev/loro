use loro_common::{ContainerID, InternalString, LoroError, LoroValue, ID};
use serde_columnar::{columnar, to_vec};
use std::borrow::Cow;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalPhase<'a> {
    pub common: Cow<'a, [u8]>,           // -> CommonArena
    pub app_state: Cow<'a, [u8]>,        // -> EncodedAppState
    pub state_arena: Cow<'a, [u8]>,      // -> TempArena<'a>
    pub additional_arena: Cow<'a, [u8]>, // -> TempArena<'a>，抛弃这部分则不能回溯历史
    pub oplog: Cow<'a, [u8]>,            // -> OpLog. Can be ignored if we only need state
}

impl<'a> FinalPhase<'a> {
    #[inline(always)]
    pub fn encode(&self) -> Vec<u8> {
        to_vec(self).unwrap()
    }

    #[inline(always)]
    pub fn decode(bytes: &'a [u8]) -> Result<Self, LoroError> {
        serde_columnar::from_bytes(bytes)
            .map_err(|e| LoroError::DecodeError(e.to_string().into_boxed_str()))
    }
}

#[columnar(ser, de)]
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CommonArena<'a> {
    pub peer_ids: Cow<'a, [u64]>,
    pub container_ids: Vec<ContainerID>,
}

impl<'a> CommonArena<'a> {
    pub fn encode(&self) -> Vec<u8> {
        to_vec(self).unwrap()
    }

    pub fn decode(data: &'a FinalPhase) -> Result<Self, LoroError> {
        serde_columnar::from_bytes(&data.common)
            .map_err(|e| LoroError::DecodeError(e.to_string().into_boxed_str()))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EncodedAppState {
    pub frontiers: Vec<ID>,
    /// container states
    pub states: Vec<EncodedContainerState>,
    /// containers' parents
    pub parents: Vec<Option<u32>>,
}

impl EncodedAppState {
    pub fn encode(&self) -> Vec<u8> {
        to_vec(self).unwrap()
    }

    pub fn decode(data: &FinalPhase) -> Result<Self, LoroError> {
        serde_columnar::from_bytes(&data.app_state)
            .map_err(|e| LoroError::DecodeError(e.to_string().into_boxed_str()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EncodedContainerState {
    Text { len: usize },
    Map(Vec<MapEntry>),
    List(Vec<usize>),
}
impl EncodedContainerState {
    pub fn container_type(&self) -> loro_common::ContainerType {
        match self {
            EncodedContainerState::Text { .. } => loro_common::ContainerType::Text,
            EncodedContainerState::Map(_) => loro_common::ContainerType::Map,
            EncodedContainerState::List(_) => loro_common::ContainerType::List,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapEntry {
    pub key: usize,   // index to the state arena
    pub value: usize, // index to the state arena + 1. 0 means None
    pub peer: u32,    // index to the peer ids
    pub counter: u32, // index to the peer ids
    pub lamport: u32,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TempArena<'a> {
    pub text: Cow<'a, [u8]>,
    pub keywords: Vec<InternalString>,
    pub values: Vec<LoroValue>,
}

impl<'a> TempArena<'a> {
    pub fn encode(&self) -> Vec<u8> {
        to_vec(self).unwrap()
    }

    pub fn decode_state_arena(data: &'a FinalPhase) -> Result<Self, LoroError> {
        serde_columnar::from_bytes(&data.state_arena)
            .map_err(|e| LoroError::DecodeError(e.to_string().into_boxed_str()))
    }

    pub fn decode_additional_arena(data: &'a FinalPhase) -> Result<Self, LoroError> {
        serde_columnar::from_bytes(&data.additional_arena)
            .map_err(|e| LoroError::DecodeError(e.to_string().into_boxed_str()))
    }
}

/// returns a deep LoroValue that wraps the whole state
pub fn decode_state(_bytes: &[u8]) -> LoroValue {
    unimplemented!()
}
