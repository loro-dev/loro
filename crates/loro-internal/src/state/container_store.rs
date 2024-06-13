use crate::{arena::SharedArena, container::idx::ContainerIdx};
use bytes::Bytes;
use encode::{decode_cids, CidOffsetEncoder};
use fxhash::FxHashMap;
use loro_common::{ContainerID, LoroResult, LoroValue};
use once_cell::sync::OnceCell;

use super::{ContainerState, State};

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

    pub fn encode(&self) -> Bytes {
        let mut id_bytes_pairs = Vec::with_capacity(self.store.len());
        for (idx, container) in self.store.iter() {
            let id = self.arena.get_container_id(*idx).unwrap();
            id_bytes_pairs.push((id, container.encode()))
        }
        id_bytes_pairs.sort_by(|(a, _), (b, _)| a.cmp(b));

        let mut id_encoder = CidOffsetEncoder::new();
        let mut offset = 0;
        for (id, bytes) in id_bytes_pairs.iter() {
            id_encoder.push(id, offset);
            offset += bytes.len();
        }

        let mut bytes = Vec::with_capacity(self.store.len() * 4 + 4);
        bytes.resize(4, 0);
        id_encoder.to_io(&mut bytes);
        // set the first 4 bytes of bytes to the length of itself
        let len = bytes.len() as u32;
        bytes[0] = (len & 0xff) as u8;
        bytes[1] = ((len >> 8) & 0xff) as u8;
        bytes[2] = ((len >> 16) & 0xff) as u8;
        bytes[3] = ((len >> 24) & 0xff) as u8;
        for (_, b) in id_bytes_pairs.iter() {
            bytes.extend_from_slice(b);
        }

        bytes.into()
    }

    pub fn decode(&mut self, bytes: Bytes) -> LoroResult<()> {
        let offset = u32::from_le_bytes((&bytes[..4]).try_into().unwrap()) as usize;
        let cids = &bytes[4..offset];
        let cids = decode_cids(cids)?;

        let container_bytes = bytes.slice(offset..);
        for (cid, offset) in cids {
            let container = ContainerWrapper::new_from_bytes(container_bytes.slice(offset..));
            let idx = self.arena.register_container(&cid);
            self.store.insert(idx, container);
        }

        Ok(())
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

    pub fn encode(&self) -> Bytes {
        todo!("encode container")
    }

    pub fn new_from_bytes(bytes: Bytes) -> Self {
        ContainerWrapper::Bytes(bytes)
    }
}

mod encode {
    use loro_common::{
        ContainerID, ContainerType, Counter, InternalString, LoroError, LoroResult, PeerID,
    };
    use serde::{Deserialize, Serialize};
    use serde_columnar::{
        izip, AnyRleDecoder, AnyRleEncoder, BoolRleDecoder, BoolRleEncoder, DeltaRleDecoder,
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

        pub fn to_io<W: Write>(self, w: W) {
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
            encoder.to_io(&mut bytes);

            let cids = decode_cids(&bytes).unwrap();
            assert_eq!(&cids, &input)
        }
    }
}
