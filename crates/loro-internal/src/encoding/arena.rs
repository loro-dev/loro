use std::ops::Deref;

use crate::InternalString;
use loro_common::{ContainerID, ContainerType, LoroError, LoroResult, PeerID};
use serde::{Deserialize, Serialize};
use serde_columnar::{columnar, ColumnarError};

use super::encode_reordered::{PeerIdx, ValueRegister, MAX_DECODED_SIZE};

pub(super) fn encode_arena(
    peer_ids_arena: Vec<u64>,
    containers: ContainerArena,
    keys: Vec<InternalString>,
    deps: DepsArena,
    state_blob_arena: &[u8],
) -> Vec<u8> {
    let peer_ids = PeerIdArena {
        peer_ids: peer_ids_arena,
    };

    let key_arena = KeyArena { keys };
    let encoded = EncodedArenas {
        peer_id_arena: &peer_ids.encode(),
        container_arena: &containers.encode(),
        key_arena: &key_arena.encode(),
        deps_arena: &deps.encode(),
        state_blob_arena,
    };

    encoded.encode_arenas()
}

pub struct EncodedRegisters {
    pub(super) peer: ValueRegister<PeerID>,
    pub(super) key: ValueRegister<InternalString>,
    pub(super) container: ValueRegister<ContainerID>,
}

pub struct DecodedArenas<'a> {
    pub(super) peer_ids: PeerIdArena,
    pub(super) containers: ContainerArena,
    pub(super) keys: KeyArena,
    pub deps: Box<dyn Iterator<Item = Result<EncodedDep, ColumnarError>> + 'a>,
    pub state_blob_arena: &'a [u8],
}

pub fn decode_arena(bytes: &[u8]) -> LoroResult<DecodedArenas> {
    let arenas = EncodedArenas::decode_arenas(bytes)?;
    Ok(DecodedArenas {
        peer_ids: PeerIdArena::decode(arenas.peer_id_arena)?,
        containers: ContainerArena::decode(arenas.container_arena)?,
        keys: KeyArena::decode(arenas.key_arena)?,
        deps: Box::new(DepsArena::decode_iter(arenas.deps_arena)?),
        state_blob_arena: arenas.state_blob_arena,
    })
}

struct EncodedArenas<'a> {
    peer_id_arena: &'a [u8],
    container_arena: &'a [u8],
    key_arena: &'a [u8],
    deps_arena: &'a [u8],
    state_blob_arena: &'a [u8],
}

impl EncodedArenas<'_> {
    fn encode_arenas(self) -> Vec<u8> {
        let mut ans = Vec::with_capacity(
            self.peer_id_arena.len()
                + self.container_arena.len()
                + self.key_arena.len()
                + self.deps_arena.len()
                + 4 * 4,
        );

        write_arena(&mut ans, self.peer_id_arena);
        write_arena(&mut ans, self.container_arena);
        write_arena(&mut ans, self.key_arena);
        write_arena(&mut ans, self.deps_arena);
        write_arena(&mut ans, self.state_blob_arena);
        ans
    }

    fn decode_arenas(bytes: &[u8]) -> LoroResult<EncodedArenas> {
        let (peer_id_arena, rest) = read_arena(bytes)?;
        let (container_arena, rest) = read_arena(rest)?;
        let (key_arena, rest) = read_arena(rest)?;
        let (deps_arena, rest) = read_arena(rest)?;
        let (state_blob_arena, _) = read_arena(rest)?;
        Ok(EncodedArenas {
            peer_id_arena,
            container_arena,
            key_arena,
            deps_arena,
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
pub(super) struct EncodedContainer {
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
    pub fn as_container_id(&self, arenas: &DecodedArenas) -> LoroResult<ContainerID> {
        if self.is_root {
            Ok(ContainerID::Root {
                container_type: ContainerType::try_from_u8(self.kind)
                    .unwrap_or(ContainerType::Unknown(self.kind)),
                name: arenas
                    .keys
                    .get(self.key_idx_or_counter as usize)
                    .ok_or(LoroError::DecodeDataCorruptionError)?
                    .clone(),
            })
        } else {
            Ok(ContainerID::Normal {
                container_type: ContainerType::try_from_u8(self.kind)
                    .unwrap_or(ContainerType::Unknown(self.kind)),
                peer: *(arenas
                    .peer_ids
                    .get(self.peer_idx)
                    .ok_or(LoroError::DecodeDataCorruptionError)?),
                counter: self.key_idx_or_counter,
            })
        }
    }
}

#[columnar(ser, de)]
#[derive(Default)]
pub(super) struct ContainerArena {
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
    fn encode(&self) -> Vec<u8> {
        serde_columnar::to_vec(&self.containers).unwrap()
    }

    fn decode(bytes: &[u8]) -> LoroResult<Self> {
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
