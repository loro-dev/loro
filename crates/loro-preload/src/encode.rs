use bytes::{BufMut, BytesMut};
use loro_common::{ContainerID, InternalString, LoroError, LoroResult, LoroValue, ID};
use serde_columnar::{columnar, to_vec};
use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// The final phase of the encoding process. It's also the first phase of the decoding process.
///
/// This data structure allows users to only load the state or the oplog.
///
/// - When only the state is needed, the `oplog` and `oplog_extra_arena` can be ignored.
/// - When only the oplog is needed, the `app_state` can be ignored. (state_arena is still needed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalPhase<'a> {
    #[serde(borrow)]
    pub common: Cow<'a, [u8]>, // -> CommonArena
    #[serde(borrow)]
    pub app_state: Cow<'a, [u8]>, // -> EncodedAppState
    #[serde(borrow)]
    pub state_arena: Cow<'a, [u8]>, // -> TempArena<'a>
    #[serde(borrow)]
    pub oplog_extra_arena: Cow<'a, [u8]>, // -> TempArena<'a>. Cannot have full history if this is dropped
    #[serde(borrow)]
    pub oplog: Cow<'a, [u8]>, // -> OpLog. Can be ignored if we only need state
}

impl<'a> FinalPhase<'a> {
    #[inline(always)]
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = BytesMut::with_capacity(
            self.common.len()
                + self.app_state.len()
                + self.state_arena.len()
                + self.oplog_extra_arena.len()
                + self.oplog.len()
                + 10,
        );

        leb::write_unsigned(&mut bytes, self.common.len() as u64);
        bytes.put_slice(&self.common);
        leb::write_unsigned(&mut bytes, self.app_state.len() as u64);
        bytes.put_slice(&self.app_state);
        leb::write_unsigned(&mut bytes, self.state_arena.len() as u64);
        bytes.put_slice(&self.state_arena);
        leb::write_unsigned(&mut bytes, self.oplog_extra_arena.len() as u64);
        bytes.put_slice(&self.oplog_extra_arena);
        leb::write_unsigned(&mut bytes, self.oplog.len() as u64);
        bytes.put_slice(&self.oplog);
        bytes.to_vec()
    }

    #[inline(always)]
    pub fn decode(bytes: &'a [u8]) -> Result<Self, LoroError> {
        let mut index = 0;
        let len = leb::read_unsigned(bytes, &mut index) as usize;
        let common = &bytes[index..index + len];
        index += len;

        let len = leb::read_unsigned(bytes, &mut index) as usize;
        let app_state = &bytes[index..index + len];
        index += len;

        let len = leb::read_unsigned(bytes, &mut index) as usize;
        let state_arena = &bytes[index..index + len];
        index += len;

        let len = leb::read_unsigned(bytes, &mut index) as usize;
        let additional_arena = &bytes[index..index + len];
        index += len;

        let len = leb::read_unsigned(bytes, &mut index) as usize;
        let oplog = &bytes[index..index + len];

        Ok(FinalPhase {
            common: Cow::Borrowed(common),
            app_state: Cow::Borrowed(app_state),
            state_arena: Cow::Borrowed(state_arena),
            oplog_extra_arena: Cow::Borrowed(additional_arena),
            oplog: Cow::Borrowed(oplog),
        })
    }

    pub fn diagnose_size(&self) {
        println!("common: {}", self.common.len());
        println!("app_state: {}", self.app_state.len());
        println!("state_arena: {}", self.state_arena.len());
        println!("additional_arena: {}", self.oplog_extra_arena.len());
        println!("oplog: {}", self.oplog.len());
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CommonArena<'a> {
    #[serde(borrow)]
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
pub struct EncodedAppState<'a> {
    pub frontiers: Vec<ID>,
    /// container states
    #[serde(borrow)]
    pub states: Vec<EncodedContainerState<'a>>,
    /// containers' parents
    pub parents: Vec<Option<u32>>,
}

impl<'a> EncodedAppState<'a> {
    pub fn encode(&self) -> Vec<u8> {
        to_vec(self).unwrap()
    }

    pub fn decode(data: &'a FinalPhase) -> Result<EncodedAppState<'a>, LoroError> {
        serde_columnar::from_bytes(&data.app_state)
            .map_err(|e| LoroError::DecodeError(e.to_string().into_boxed_str()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EncodedContainerState<'a> {
    Map(Vec<MapEntry>),
    List {
        elem_idx: Vec<usize>,
        elem_ids: Vec<ID>,
    },
    #[serde(borrow)]
    Richtext(Box<EncodedRichtextState<'a>>),
    Tree((Vec<(usize, Option<usize>)>, Vec<usize>)),
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct EncodedRichtextState<'a> {
    /// It's composed of interleaved:
    ///
    /// - len of text ranges
    /// - len of styles anchors
    pub len: Vec<u32>,
    /// This is encoded [TextRanges]
    #[serde(borrow)]
    pub text_bytes: Cow<'a, [u8]>,
    pub ids: Vec<(u32, u32)>,
    /// Style anchor index in the style arena
    // TODO: can be optimized
    pub styles: Vec<CompactStyleOp>,
    /// It is a start or end anchor. It's indexed by bit position.
    pub is_style_start: Vec<u8>,
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone, Copy)]
pub struct TextRange {
    #[columnar(strategy = "DeltaRle")]
    pub start: usize,
    #[columnar(strategy = "DeltaRle")]
    pub len: usize,
}

#[columnar(ser, de)]
#[derive(Debug, Default)]
pub struct TextRanges {
    #[columnar(class = "vec", iter = "TextRange")]
    pub ranges: Vec<TextRange>,
}

impl TextRanges {
    #[inline]
    pub fn decode_iter(bytes: &[u8]) -> LoroResult<impl Iterator<Item = TextRange> + '_> {
        let iter = serde_columnar::iter_from_bytes::<TextRanges>(bytes)?;
        Ok(iter.ranges)
    }

    #[inline]
    pub fn encode(&self) -> Vec<u8> {
        to_vec(self).unwrap()
    }
}

impl<'a> EncodedContainerState<'a> {
    pub fn container_type(&self) -> loro_common::ContainerType {
        match self {
            EncodedContainerState::Map(_) => loro_common::ContainerType::Map,
            EncodedContainerState::List { .. } => loro_common::ContainerType::List,
            EncodedContainerState::Tree(_) => loro_common::ContainerType::Tree,
            EncodedContainerState::Richtext { .. } => loro_common::ContainerType::Text,
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
pub struct CompactStyleOp {
    /// index to the peer idx
    pub peer_idx: u32,
    /// index to the keywords idx
    pub key_idx: u32,
    pub counter: u32,
    pub lamport: u32,
    pub style_info: u8,
    pub value: LoroValue,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TempArena<'a> {
    #[serde(borrow)]
    pub text: Cow<'a, [u8]>,
    // PERF: can we use a Cow here?
    pub keywords: Vec<InternalString>,
    pub values: Vec<LoroValue>,
    pub tree_ids: Vec<(u32, i32)>,
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
        serde_columnar::from_bytes(&data.oplog_extra_arena)
            .map_err(|e| LoroError::DecodeError(e.to_string().into_boxed_str()))
    }
}

/// returns a deep LoroValue that wraps the whole state
pub fn decode_state(_bytes: &[u8]) -> LoroValue {
    unimplemented!()
}

mod leb {
    use bytes::{BufMut, BytesMut};
    pub const CONTINUATION_BIT: u8 = 1 << 7;

    pub fn write_unsigned(w: &mut BytesMut, mut val: u64) -> usize {
        let mut bytes_written = 0;
        loop {
            let mut byte = low_bits_of_u64(val);
            val >>= 7;
            if val != 0 {
                // More bytes to come, so set the continuation bit.
                byte |= CONTINUATION_BIT;
            }

            w.put_u8(byte);
            bytes_written += 1;

            if val == 0 {
                return bytes_written;
            }
        }
    }

    #[doc(hidden)]
    #[inline]
    pub fn low_bits_of_byte(byte: u8) -> u8 {
        byte & !CONTINUATION_BIT
    }

    #[doc(hidden)]
    #[inline]
    pub fn low_bits_of_u64(val: u64) -> u8 {
        let byte = val & (std::u8::MAX as u64);
        low_bits_of_byte(byte as u8)
    }

    pub fn read_unsigned(r: &[u8], index: &mut usize) -> u64 {
        let mut result = 0;
        let mut shift = 0;

        loop {
            let mut buf = [r[*index]];
            *index += 1;

            if shift == 63 && buf[0] != 0x00 && buf[0] != 0x01 {
                while buf[0] & CONTINUATION_BIT != 0 {
                    buf = [r[*index]];
                    *index += 1;
                }

                panic!("overflow");
            }

            let low_bits = low_bits_of_byte(buf[0]) as u64;
            result |= low_bits << shift;

            if buf[0] & CONTINUATION_BIT == 0 {
                return result;
            }

            shift += 7;
        }
    }
}
