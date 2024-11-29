use std::{borrow::Cow, ops::Deref};

use crate::InternalString;
use fxhash::FxHashSet;
use itertools::Itertools;
use loro_common::{ContainerID, ContainerType, Counter, LoroError, LoroResult, PeerID, ID};
use serde::{Deserialize, Serialize};
use serde_columnar::{columnar, ColumnarError};

use super::{
    outdated_encode_reordered::{PeerIdx, MAX_DECODED_SIZE},
    value::{Value, ValueDecodedArenasTrait, ValueEncodeRegister},
    value_register::ValueRegister,
};

pub(super) fn encode_arena(
    registers: EncodedRegisters,
    dep_arena: DepsArena,
    state_blob_arena: &[u8],
) -> Vec<u8> {
    let EncodedRegisters {
        peer: mut peer_register,
        container: cid_register,
        key: mut key_register,
        tree_id: tree_id_register,
        position: position_register,
    } = registers;

    let container_arena = ContainerArena::from_containers(
        cid_register.unwrap_vec(),
        &mut peer_register,
        &mut key_register,
    );

    let position_arena =
        PositionArena::from_positions(position_register.right().unwrap().unwrap_vec());
    let tree_id_arena = TreeIDArena {
        tree_ids: tree_id_register.unwrap_vec(),
    };
    let peer_ids = PeerIdArena {
        peer_ids: peer_register.unwrap_vec(),
    };

    let key_arena = KeyArena {
        keys: key_register.unwrap_vec(),
    };
    let encoded = EncodedArenas {
        peer_id_arena: &peer_ids.encode(),
        container_arena: &container_arena.encode(),
        key_arena: &key_arena.encode(),
        deps_arena: &dep_arena.encode(),
        position_arena: &position_arena.encode(),
        tree_id_arena: &tree_id_arena.encode(),
        state_blob_arena,
    };

    encoded.encode_arenas()
}

#[derive(Debug)]
pub struct EncodedRegisters<'a> {
    pub(super) peer: ValueRegister<PeerID>,
    pub(super) key: ValueRegister<InternalString>,
    pub(super) container: ValueRegister<ContainerID>,
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

impl EncodedRegisters<'_> {
    pub(crate) fn sort_fractional_index(&mut self) {
        let position_register =
            std::mem::replace(&mut self.position, either::Left(Default::default()))
                .left()
                .unwrap();
        let positions = position_register.into_iter().sorted_unstable().collect();
        let position_register = ValueRegister::from_existing(positions);
        self.position = either::Right(position_register);
    }
}

pub struct DecodedArenas<'a> {
    pub(super) peer_ids: PeerIdArena,
    pub(super) containers: ContainerArena,
    pub(super) keys: KeyArena,
    pub deps: Box<dyn Iterator<Item = Result<EncodedDep, ColumnarError>> + 'a>,
    pub(super) positions: PositionArena<'a>,
    pub(super) tree_ids: TreeIDArena,
    pub state_blob_arena: &'a [u8],
}

impl ValueDecodedArenasTrait for DecodedArenas<'_> {
    fn keys(&self) -> &[InternalString] {
        &self.keys.keys
    }

    fn peers(&self) -> &[PeerID] {
        &self.peer_ids.peer_ids
    }

    fn decode_tree_op(
        &self,
        positions: &[Vec<u8>],
        op: super::value::EncodedTreeMove,
        id: ID,
    ) -> LoroResult<crate::container::tree::tree_op::TreeOp> {
        op.as_tree_op(
            &self.peer_ids.peer_ids,
            positions,
            &self.tree_ids.tree_ids,
            id,
        )
    }
}

pub fn decode_arena(bytes: &[u8]) -> LoroResult<DecodedArenas> {
    let arenas = EncodedArenas::decode_arenas(bytes)?;
    Ok(DecodedArenas {
        peer_ids: PeerIdArena::decode(arenas.peer_id_arena)?,
        containers: ContainerArena::decode(arenas.container_arena)?,
        keys: KeyArena::decode(arenas.key_arena)?,
        deps: Box::new(DepsArena::decode_iter(arenas.deps_arena)?),
        positions: PositionArena::decode(arenas.position_arena)?,
        tree_ids: TreeIDArena::decode(arenas.tree_id_arena)?,
        state_blob_arena: arenas.state_blob_arena,
    })
}

struct EncodedArenas<'a> {
    peer_id_arena: &'a [u8],
    container_arena: &'a [u8],
    key_arena: &'a [u8],
    deps_arena: &'a [u8],
    position_arena: &'a [u8],
    tree_id_arena: &'a [u8],
    state_blob_arena: &'a [u8],
}

impl EncodedArenas<'_> {
    fn encode_arenas(self) -> Vec<u8> {
        let mut ans = Vec::with_capacity(
            self.peer_id_arena.len()
                + self.container_arena.len()
                + self.key_arena.len()
                + self.deps_arena.len()
                + self.position_arena.len()
                + self.tree_id_arena.len()
                + 4 * 4,
        );

        write_arena(&mut ans, self.peer_id_arena);
        write_arena(&mut ans, self.container_arena);
        write_arena(&mut ans, self.key_arena);
        write_arena(&mut ans, self.deps_arena);
        write_arena(&mut ans, self.position_arena);
        write_arena(&mut ans, self.tree_id_arena);
        write_arena(&mut ans, self.state_blob_arena);
        ans
    }

    fn decode_arenas(bytes: &[u8]) -> LoroResult<EncodedArenas> {
        let (peer_id_arena, rest) = read_arena(bytes)?;
        let (container_arena, rest) = read_arena(rest)?;
        let (key_arena, rest) = read_arena(rest)?;
        let (deps_arena, rest) = read_arena(rest)?;
        let (position_arena, rest) = read_arena(rest)?;
        let (tree_id_arena, rest) = read_arena(rest)?;
        let (state_blob_arena, _) = read_arena(rest)?;
        Ok(EncodedArenas {
            peer_id_arena,
            container_arena,
            key_arena,
            deps_arena,
            position_arena,
            tree_id_arena,
            state_blob_arena,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct PeerIdArena {
    pub(super) peer_ids: Vec<u64>,
}

impl Deref for PeerIdArena {
    type Target = [u64];

    fn deref(&self) -> &Self::Target {
        &self.peer_ids
    }
}

impl PeerIdArena {
    fn encode(&self) -> Vec<u8> {
        let mut ans = Vec::with_capacity(self.peer_ids.len() * 8);
        leb128::write::unsigned(&mut ans, self.peer_ids.len() as u64).unwrap();
        for &peer_id in &self.peer_ids {
            ans.extend_from_slice(&peer_id.to_be_bytes());
        }
        ans
    }

    fn decode(peer_id_arena: &[u8]) -> LoroResult<Self> {
        let mut reader = peer_id_arena;
        let len = leb128::read::unsigned(&mut reader)
            .map_err(|_| LoroError::DecodeDataCorruptionError)?;
        if len > MAX_DECODED_SIZE as u64 {
            return Err(LoroError::DecodeDataCorruptionError);
        }

        let mut peer_ids = Vec::with_capacity(len as usize);
        if reader.len() < len as usize * 8 {
            return Err(LoroError::DecodeDataCorruptionError);
        }

        for _ in 0..len {
            let mut peer_id_bytes = [0; 8];
            peer_id_bytes.copy_from_slice(&reader[..8]);
            peer_ids.push(u64::from_be_bytes(peer_id_bytes));
            reader = &reader[8..];
        }
        Ok(PeerIdArena { peer_ids })
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

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct EncodedDep {
    #[columnar(strategy = "Rle")]
    pub peer_idx: usize,
    #[columnar(strategy = "DeltaRle")]
    pub counter: i32,
}

#[columnar(ser, de)]
#[derive(Default)]
pub(super) struct DepsArena {
    #[columnar(class = "vec", iter = "EncodedDep")]
    deps: Vec<EncodedDep>,
}

impl Deref for DepsArena {
    type Target = [EncodedDep];

    fn deref(&self) -> &Self::Target {
        &self.deps
    }
}

impl DepsArena {
    pub fn push(&mut self, peer_idx: PeerIdx, counter: i32) {
        self.deps.push(EncodedDep { peer_idx, counter });
    }

    pub fn encode(&self) -> Vec<u8> {
        serde_columnar::to_vec(&self).unwrap()
    }

    pub fn decode_iter(
        bytes: &[u8],
    ) -> LoroResult<impl Iterator<Item = Result<EncodedDep, ColumnarError>> + '_> {
        let iter = serde_columnar::iter_from_bytes::<DepsArena>(bytes)?;
        Ok(iter.deps)
    }
}

#[derive(Serialize, Deserialize, Default)]
pub(super) struct KeyArena {
    pub(super) keys: Vec<InternalString>,
}

impl Deref for KeyArena {
    type Target = [InternalString];

    fn deref(&self) -> &Self::Target {
        &self.keys
    }
}

impl KeyArena {
    pub fn encode(&self) -> Vec<u8> {
        serde_columnar::to_vec(&self).unwrap()
    }

    pub fn decode(bytes: &[u8]) -> LoroResult<Self> {
        Ok(serde_columnar::from_bytes(bytes)?)
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
#[columnar(vec, ser, de)]
pub struct TreeIDArena {
    #[columnar(class = "vec", iter = "EncodedTreeID")]
    pub(super) tree_ids: Vec<EncodedTreeID>,
}

impl TreeIDArena {
    pub fn decode(bytes: &[u8]) -> LoroResult<Self> {
        Ok(serde_columnar::from_bytes(bytes)?)
    }

    pub fn encode(&self) -> Vec<u8> {
        serde_columnar::to_vec(&self).unwrap()
    }
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

fn write_arena(buffer: &mut Vec<u8>, arena: &[u8]) {
    leb128::write::unsigned(buffer, arena.len() as u64).unwrap();
    buffer.extend_from_slice(arena);
}

/// Return (next_arena, rest)
fn read_arena(mut buffer: &[u8]) -> LoroResult<(&[u8], &[u8])> {
    let reader = &mut buffer;
    let len =
        leb128::read::unsigned(reader).map_err(|_| LoroError::DecodeDataCorruptionError)? as usize;
    if len > MAX_DECODED_SIZE {
        return Err(LoroError::DecodeDataCorruptionError);
    }

    if len > reader.len() {
        return Err(LoroError::DecodeDataCorruptionError);
    }

    Ok((reader[..len as usize].as_ref(), &reader[len as usize..]))
}
