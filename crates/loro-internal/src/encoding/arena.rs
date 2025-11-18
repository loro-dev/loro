use std::{borrow::Cow, ops::Deref};

use crate::InternalString;
use loro_common::{ContainerID, ContainerType, Counter, LoroError, LoroResult, PeerID};
use rustc_hash::FxHashSet;
use serde_columnar::columnar;

use super::{
    outdated_encode_reordered::PeerIdx,
    value::{Value, ValueDecodedArenasTrait, ValueEncodeRegister},
    value_register::ValueRegister,
};

#[derive(Debug)]
pub struct EncodedRegisters<'a> {
    pub(super) peer: ValueRegister<PeerID>,
    pub(super) key: ValueRegister<InternalString>,
    pub(super) tree_id: ValueRegister<EncodedTreeID>,
    pub(super) position: either::Either<FxHashSet<&'a [u8]>, ValueRegister<&'a [u8]>>,
}

impl ValueEncodeRegister for EncodedRegisters<'_> {
    fn key_mut(&mut self) -> &mut ValueRegister<InternalString> {
        &mut self.key
    }

    fn peer_mut(&mut self) -> &mut ValueRegister<PeerID> {
        &mut self.peer
    }

    fn encode_tree_op(
        &mut self,
        op: &crate::container::tree::tree_op::TreeOp,
    ) -> super::value::Value<'static> {
        Value::TreeMove(super::value::EncodedTreeMove::from_tree_op(op, self))
    }
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct EncodedContainer {
    #[columnar(strategy = "BoolRle")]
    is_root: bool,
    #[columnar(strategy = "Rle")]
    kind: u8,
    #[columnar(strategy = "Rle")]
    peer_idx: usize,
    #[columnar(strategy = "DeltaRle")]
    key_idx_or_counter: i32,
}

impl EncodedContainer {
    pub fn as_container_id(&self, arenas: &dyn ValueDecodedArenasTrait) -> LoroResult<ContainerID> {
        if self.is_root {
            Ok(ContainerID::Root {
                container_type: ContainerType::try_from_u8(self.kind)
                    .unwrap_or(ContainerType::Unknown(self.kind)),
                name: arenas
                    .keys()
                    .get(self.key_idx_or_counter as usize)
                    .ok_or(LoroError::DecodeDataCorruptionError)?
                    .clone(),
            })
        } else {
            Ok(ContainerID::Normal {
                container_type: ContainerType::try_from_u8(self.kind)
                    .unwrap_or(ContainerType::Unknown(self.kind)),
                peer: *(arenas
                    .peers()
                    .get(self.peer_idx)
                    .ok_or(LoroError::DecodeDataCorruptionError)?),
                counter: self.key_idx_or_counter,
            })
        }
    }
}

#[columnar(ser, de)]
#[derive(Default)]
pub(crate) struct ContainerArena {
    #[columnar(class = "vec", iter = "EncodedContainer")]
    pub(super) containers: Vec<EncodedContainer>,
}

impl Deref for ContainerArena {
    type Target = [EncodedContainer];

    fn deref(&self) -> &Self::Target {
        &self.containers
    }
}

impl ContainerArena {
    pub fn encode(&self) -> Vec<u8> {
        serde_columnar::to_vec(&self.containers).unwrap()
    }

    pub fn decode(bytes: &[u8]) -> LoroResult<Self> {
        Ok(ContainerArena {
            containers: serde_columnar::from_bytes(bytes)?,
        })
    }

    pub fn from_containers(
        cids: Vec<ContainerID>,
        peer_register: &mut ValueRegister<PeerID>,
        key_reg: &mut ValueRegister<InternalString>,
    ) -> Self {
        let mut ans = Self {
            containers: Vec::with_capacity(cids.len()),
        };
        for cid in cids {
            ans.push(cid, peer_register, key_reg);
        }

        ans
    }

    pub fn push(
        &mut self,
        id: ContainerID,
        peer_register: &mut ValueRegister<PeerID>,
        register_key: &mut ValueRegister<InternalString>,
    ) {
        let (is_root, kind, peer_idx, key_idx_or_counter) = match id {
            ContainerID::Root {
                container_type,
                name,
            } => (true, container_type, 0, register_key.register(&name) as i32),
            ContainerID::Normal {
                container_type,
                peer,
                counter,
            } => (
                false,
                container_type,
                peer_register.register(&peer),
                counter,
            ),
        };
        self.containers.push(EncodedContainer {
            is_root,
            kind: kind.to_u8(),
            peer_idx,
            key_idx_or_counter,
        });
    }
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
#[columnar(vec, ser, de, iterable)]
pub struct EncodedTreeID {
    #[columnar(strategy = "Rle")]
    pub peer_idx: PeerIdx,
    #[columnar(strategy = "DeltaRle")]
    pub counter: Counter,
}

#[derive(Clone)]
#[columnar(vec, ser, de, iterable)]
pub(super) struct PositionDelta<'a> {
    #[columnar(strategy = "Rle")]
    common_prefix_length: usize,
    #[columnar(borrow)]
    rest: Cow<'a, [u8]>,
}

#[derive(Default)]
#[columnar(ser, de)]
pub(crate) struct PositionArena<'a> {
    #[columnar(class = "vec", iter = "PositionDelta<'a>")]
    pub(super) positions: Vec<PositionDelta<'a>>,
}

impl<'a> PositionArena<'a> {
    pub fn from_positions(positions: impl IntoIterator<Item = &'a [u8]>) -> Self {
        let iter = positions.into_iter();
        let mut ans = Vec::with_capacity(iter.size_hint().0);
        let mut last_bytes: &[u8] = &[];
        for p in iter {
            let common = longest_common_prefix_length(last_bytes, p);
            let rest = &p[common..];
            last_bytes = p;
            ans.push(PositionDelta {
                common_prefix_length: common,
                rest: Cow::Borrowed(rest),
            })
        }
        Self { positions: ans }
    }

    pub fn parse_to_positions(self) -> Vec<Vec<u8>> {
        let mut ans: Vec<Vec<u8>> = Vec::with_capacity(self.positions.len());
        for PositionDelta {
            common_prefix_length,
            rest,
        } in self.positions
        {
            // +1 for Fractional Index
            let mut p = Vec::with_capacity(rest.len() + common_prefix_length + 1);
            if let Some(last_bytes) = ans.last() {
                p.extend_from_slice(&last_bytes[0..common_prefix_length]);
            }
            p.extend_from_slice(rest.as_ref());
            ans.push(p);
        }
        ans
    }

    pub fn encode(&self) -> Vec<u8> {
        serde_columnar::to_vec(&self).unwrap()
    }

    pub fn decode<'de: 'a>(bytes: &'de [u8]) -> LoroResult<Self> {
        Ok(serde_columnar::from_bytes(bytes)?)
    }

    pub fn encode_v2(&self) -> Vec<u8> {
        if self.positions.is_empty() {
            return Vec::new();
        }

        serde_columnar::to_vec(&self).unwrap()
    }

    pub fn decode_v2<'de: 'a>(bytes: &'de [u8]) -> LoroResult<Self> {
        if bytes.is_empty() {
            return Ok(Self::default());
        }

        Ok(serde_columnar::from_bytes(bytes)?)
    }
}

fn longest_common_prefix_length(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}
