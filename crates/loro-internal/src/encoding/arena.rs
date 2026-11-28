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
    pub(crate) is_root: bool,
    #[columnar(strategy = "Rle")]
    pub(crate) kind: u8,
    #[columnar(strategy = "Rle")]
    pub(crate) peer_idx: usize,
    #[columnar(strategy = "DeltaRle")]
    pub(crate) key_idx_or_counter: i32,
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
        cids: &[ContainerID],
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
        id: &ContainerID,
        peer_register: &mut ValueRegister<PeerID>,
        register_key: &mut ValueRegister<InternalString>,
    ) {
        self.containers
            .push(EncodedContainer::register(id, peer_register, register_key));
    }
}

impl EncodedContainer {
    pub(crate) fn register(
        id: &ContainerID,
        peer_register: &mut ValueRegister<PeerID>,
        register_key: &mut ValueRegister<InternalString>,
    ) -> Self {
        let (is_root, kind, peer_idx, key_idx_or_counter) = match id {
            ContainerID::Root {
                container_type,
                name,
            } => (true, container_type, 0, register_key.register(name) as i32),
            ContainerID::Normal {
                container_type,
                peer,
                counter,
            } => (
                false,
                container_type,
                peer_register.register(peer),
                *counter,
            ),
        };
        Self {
            is_root,
            kind: kind.to_u8(),
            peer_idx,
            key_idx_or_counter,
        }
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

    pub fn try_parse_to_positions(self) -> LoroResult<Vec<Vec<u8>>> {
        let mut ans: Vec<Vec<u8>> = Vec::with_capacity(self.positions.len());
        for PositionDelta {
            common_prefix_length,
            rest,
        } in self.positions
        {
            if let Some(last_bytes) = ans.last() {
                if common_prefix_length > last_bytes.len() {
                    return Err(LoroError::DecodeError(
                        "Decode position arena failed: common prefix out of range".into(),
                    ));
                }
            } else if common_prefix_length != 0 {
                return Err(LoroError::DecodeError(
                    "Decode position arena failed: first common prefix is nonzero".into(),
                ));
            }

            // +1 for Fractional Index
            let capacity = rest
                .len()
                .checked_add(common_prefix_length)
                .and_then(|len| len.checked_add(1))
                .ok_or_else(|| {
                    LoroError::DecodeError(
                        "Decode position arena failed: position length overflow".into(),
                    )
                })?;
            let mut p = Vec::with_capacity(capacity);
            if let Some(last_bytes) = ans.last() {
                p.extend_from_slice(&last_bytes[0..common_prefix_length]);
            }
            p.extend_from_slice(rest.as_ref());
            ans.push(p);
        }
        Ok(ans)
    }

    pub fn encode(&self) -> Vec<u8> {
        serde_columnar::to_vec(&self).unwrap()
    }

    pub fn decode<'de: 'a>(bytes: &'de [u8]) -> LoroResult<Self> {
        Self::decode_columns(bytes)
    }

    /// Decode the arena one row at a time instead of expanding each column in
    /// full.
    ///
    /// `common_prefix_length` is run-length encoded, so a few bytes can declare
    /// up to `serde_columnar`'s `MAX_RLE_COUNT` (10^9) rows, and the eager
    /// decoder (`serde_columnar::from_bytes`) materialized all of them before
    /// zipping the columns (16 GiB from a 207-byte update, loro-dev/loro#1349).
    /// Every `rest` row is a length-prefixed byte string, so it takes at least
    /// one input byte, and decoding both columns in lockstep bounds the row
    /// count by the input length. The columns must end together; the eager
    /// decoder silently dropped the rows of the longer one.
    fn decode_columns<'de: 'a>(bytes: &'de [u8]) -> LoroResult<Self> {
        let IterablePositionDelta {
            mut common_prefix_length,
            mut rest,
        } = serde_columnar::iter_from_bytes::<PositionArena<'de>>(bytes)?.positions;
        let mut positions = Vec::new();
        loop {
            match (
                common_prefix_length.next().transpose()?,
                rest.next().transpose()?,
            ) {
                (Some(common_prefix_length), Some(rest)) => positions.push(PositionDelta {
                    common_prefix_length,
                    rest,
                }),
                (None, None) => break,
                _ => {
                    return Err(LoroError::DecodeError(
                        "Decode position arena failed: column lengths differ".into(),
                    ))
                }
            }
        }
        Ok(Self { positions })
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

        Self::decode_columns(bytes)
    }
}

fn longest_common_prefix_length(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::{PositionArena, PositionDelta};

    #[test]
    fn position_arena_rejects_invalid_common_prefix() {
        let arena = PositionArena {
            positions: vec![PositionDelta {
                common_prefix_length: 1,
                rest: Cow::Borrowed(&[]),
            }],
        };

        assert!(arena.try_parse_to_positions().is_err());
    }

    /// A `PositionArena` table with hand-written columns: a single run of
    /// `run_len` copies of `common_prefix_length = 0`, and one `rest` per
    /// entry of `rests`.
    fn arena_bytes(run_len: isize, rests: &[&[u8]]) -> Vec<u8> {
        let common_prefix_length = postcard::to_allocvec(&(run_len, 0usize)).unwrap();
        let rest = postcard::to_allocvec(rests).unwrap();
        postcard::to_allocvec(&(
            1u8,
            [common_prefix_length.as_slice(), rest.as_slice()].as_slice(),
        ))
        .unwrap()
    }

    #[test]
    fn position_arena_round_trips() {
        let positions: [&[u8]; 3] = [b"\x10", b"\x10\x80", b"\x20"];
        let bytes = PositionArena::from_positions(positions).encode_v2();
        let decoded = PositionArena::decode_v2(&bytes)
            .unwrap()
            .try_parse_to_positions()
            .unwrap();
        assert_eq!(decoded, positions.map(|p| p.to_vec()));
        let decoded = PositionArena::decode(&bytes)
            .unwrap()
            .try_parse_to_positions()
            .unwrap();
        assert_eq!(decoded, positions.map(|p| p.to_vec()));

        let bytes = arena_bytes(2, &[b"a", b"b"]);
        let decoded = PositionArena::decode_v2(&bytes).unwrap().positions;
        assert_eq!(decoded.len(), 2);
    }

    /// loro-dev/loro#1349: a run of 10^9 prefix lengths next to one `rest` was
    /// expanded in full (16 GiB) before the columns were zipped.
    #[test]
    fn position_arena_rejects_a_run_longer_than_its_rest_column() {
        let bytes = arena_bytes(1_000_000_000, &[b"a"]);
        assert!(bytes.len() < 32);
        assert!(PositionArena::decode_v2(&bytes).is_err());
        assert!(PositionArena::decode(&bytes).is_err());
    }

    #[test]
    fn position_arena_rejects_columns_of_different_lengths() {
        for (run_len, rests) in [
            (1, &[b"a".as_slice(), b"b"][..]),
            (2, &[b"a".as_slice()][..]),
        ] {
            let bytes = arena_bytes(run_len, rests);
            assert!(
                PositionArena::decode_v2(&bytes).is_err(),
                "{run_len} {rests:?}"
            );
        }
    }
}
