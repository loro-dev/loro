//! # Encode
//!
//! ```log
//! ≈4KB after compression
//!
//!  N = Number of Changes
//!
//!  Peer_1 = This Peer
//!
//!
//! ┌──────────┬─────────────────────────┬─────────────────────────┐
//! │2B Version│  LEB Counter Start&Len  │  LEB Lamport Start&Len  │◁───┐
//! └──────────┴─────────────────────────┴─────────────────────────┘    │
//! ┌──────────────┬──────────────┬────────────────────────────────┐    │
//! │    LEB N     │ LEB Peer Num │      8B Peer_1,...,Peer_x      │◁───┤
//! └──────────────┴──────────────┴────────────────────────────────┘    │
//! ┌──────────────────────────────────────────────────────────────┐    │
//! │                   N LEB128 Change AtomLen                    │◁───┼─────  Important metadata
//! └──────────────────────────────────────────────────────────────┘    │
//! ┌───────────────────┬────────────────────────┬─────────────────┐    │
//! │N DepOnSelf BoolRle│ N Delta Rle Deps Lens  │     Dep IDs     │◁───┤
//! └───────────────────┴────────────────────────┴─────────────────┘    │
//! ┌──────────────────────────────────────────────────────────────┐    │
//! │                   N LEB128 Delta Lamports                    │◁───┘
//! └──────────────────────────────────────────────────────────────┘
//! ┌──────────────────────────────────────────────────────────────┐
//! │                N LEB128 Delta Rle Timestamps                 │
//! └──────────────────────────────────────────────────────────────┘
//! ┌────────────────────────────────┬─────────────────────────────┐
//! │    N Rle Commit Msg Lengths    │       Commit Messages       │
//! └────────────────────────────────┴─────────────────────────────┘
//!
//!  ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ Encoded Operations ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─
//!
//! ┌────────────────────┬─────────────────────────────────────────┐
//! │ ContainerIDs Size  │             ContainerIDs                │
//! └────────────────────┴─────────────────────────────────────────┘
//! ┌────────────────────┬─────────────────────────────────────────┐
//! │  Key Strings Size  │               Key Strings               │
//! └────────────────────┴─────────────────────────────────────────┘
//! ┌────────────────────┬─────────────────────────────────────────┐
//! │  Position Size     │               Position                  │
//! └────────────────────┴─────────────────────────────────────────┘
//! ┌────────┬──────────┬──────────┬───────┬───────────────────────┐
//! │        │          │          │       │                       │
//! │        │          │          │       │                       │
//! │        │          │          │       │                       │
//! │  Ops   │  LEB128  │   RLE    │ Delta │                       │
//! │  Size  │ Lengths  │Containers│  RLE  │       ValueType       │
//! │        │          │          │ Props │                       │
//! │        │          │          │       │                       │
//! │        │          │          │       │                       │
//! │        │          │          │       │                       │
//! └────────┴──────────┴──────────┴───────┴───────────────────────┘
//! ┌────────────────┬─────────────────────────────────────────────┐
//! │                │                                             │
//! │Value Bytes Size│                Value Bytes                  │
//! │                │                                             │
//! └────────────────┴─────────────────────────────────────────────┘
//! ┌──────────────────────────────────────────────────────────────┐
//! │                                                              │
//! │                       Delete Start IDs                       │
//! │                                                              │
//! └──────────────────────────────────────────────────────────────┘
//! ```
//!
//!

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::io::Write;
use std::sync::Arc;

use fractional_index::FractionalIndex;
use loro_common::{
    ContainerID, Counter, HasCounterSpan, HasLamportSpan, InternalString, Lamport, LoroError,
    LoroResult, PeerID, TreeID, ID,
};
use once_cell::sync::OnceCell;
use rle::HasLength;
use serde::{Deserialize, Serialize};
use serde_columnar::{columnar, DeltaRleDecoder, Itertools};

use super::delta_rle_encode::{UnsignedDeltaDecoder, UnsignedDeltaEncoder};
use crate::arena::SharedArena;
use crate::change::{Change, Timestamp};
use crate::container::tree::tree_op;
use crate::encoding::arena::{ContainerArena, PositionArena};
use crate::encoding::value_register::ValueRegister;
use crate::encoding::{
    self, decode_op, encode_op, get_op_prop, EncodedDeleteStartId, IterableEncodedDeleteStartId,
};
use crate::op::Op;
use serde_columnar::{
    AnyRleDecoder, AnyRleEncoder, BoolRleDecoder, BoolRleEncoder, DeltaRleEncoder,
};

#[derive(Debug, Serialize, Deserialize)]
struct EncodedBlock<'a> {
    version: u16,
    counter_start: u32,
    counter_len: u32,
    lamport_start: u32,
    lamport_len: u32,
    n_changes: u32,
    first_counter: u32,
    #[serde(borrow)]
    peers: Cow<'a, [u8]>,
    #[serde(borrow)]
    lengths: Cow<'a, [u8]>,
    #[serde(borrow)]
    dep_on_self: Cow<'a, [u8]>,
    #[serde(borrow)]
    dep_len: Cow<'a, [u8]>,
    #[serde(borrow)]
    dep_peer_idxs: Cow<'a, [u8]>,
    #[serde(borrow)]
    dep_counters: Cow<'a, [u8]>,
    #[serde(borrow)]
    lamports: Cow<'a, [u8]>,
    #[serde(borrow)]
    timestamps: Cow<'a, [u8]>,
    #[serde(borrow)]
    commit_msg_lengths: Cow<'a, [u8]>,
    #[serde(borrow)]
    commit_msgs: Cow<'a, [u8]>,
    // ---------------------- Ops ----------------------
    #[serde(borrow)]
    cids: Cow<'a, [u8]>,
    #[serde(borrow)]
    keys: Cow<'a, [u8]>,
    #[serde(borrow)]
    positions: Cow<'a, [u8]>,
    #[serde(borrow)]
    ops: Cow<'a, [u8]>,
    #[serde(borrow)]
    values: Cow<'a, [u8]>,
}

const VERSION: u16 = 0;

// MARK: encode_block
/// It's assume that every change in the block share the same peer.
pub fn encode_block(block: &[Change], arena: &SharedArena) -> Vec<u8> {
    if block.is_empty() {
        panic!("Empty block")
    }

    let mut peer_register: ValueRegister<PeerID> = ValueRegister::new();
    let peer = block[0].peer();
    peer_register.register(&peer);

    let cid_register: ValueRegister<ContainerID> = ValueRegister::new();
    let mut timestamp_encoder = DeltaRleEncoder::new();
    let mut lamport_encoder = UnsignedDeltaEncoder::new(block.len() * 2 + 4);
    let mut commit_msg_len_encoder = AnyRleEncoder::<u32>::new();
    let mut commit_msgs = String::new();
    let mut dep_self_encoder = BoolRleEncoder::new();
    let mut dep_len_encoder = AnyRleEncoder::<u64>::new();
    let mut encoded_deps = EncodedDeps {
        peer_idx: AnyRleEncoder::new(),
        counter: AnyRleEncoder::new(),
    };
    for c in block {
        timestamp_encoder.append(c.timestamp()).unwrap();
        lamport_encoder.push(c.lamport() as u64);
        if let Some(msg) = c.commit_msg.as_ref() {
            commit_msg_len_encoder.append(msg.len() as u32).unwrap();
            commit_msgs.push_str(msg);
        } else {
            commit_msg_len_encoder.append(0).unwrap();
        }

        let mut dep_on_self = false;
        for dep in c.deps().iter() {
            if dep.peer == peer {
                dep_on_self = true;
            } else {
                let peer_idx = peer_register.register(&dep.peer);
                encoded_deps.peer_idx.append(peer_idx as u32).unwrap();
                encoded_deps.counter.append(dep.counter as u32).unwrap();
            }
        }

        dep_self_encoder.append(dep_on_self).unwrap();
        dep_len_encoder
            .append(if dep_on_self {
                c.deps().len() as u64 - 1
            } else {
                c.deps().len() as u64
            })
            .unwrap();
    }

    let mut encoded_ops = Vec::new();
    let mut registers = Registers {
        peer_register,
        key_register: ValueRegister::new(),
        cid_register,
        position_register: ValueRegister::new(),
    };

    {
        // Init position register, making it ordered by fractional index
        let mut position_set = BTreeSet::default();
        for c in block {
            for op in c.ops().iter() {
                if let crate::op::InnerContent::Tree(tree_op) = &op.content {
                    match &**tree_op {
                        tree_op::TreeOp::Create { position, .. } => {
                            position_set.insert(position.clone());
                        }
                        tree_op::TreeOp::Move { position, .. } => {
                            position_set.insert(position.clone());
                        }
                        tree_op::TreeOp::Delete { .. } => {}
                    }
                }
            }
        }

        for position in position_set {
            registers.position_register.register(&position);
        }
    }

    let mut del_starts: Vec<_> = Vec::new();
    let mut value_writer = ValueWriter::new();
    for c in block {
        for op in c.ops().iter() {
            let cid = arena.get_container_id(op.container).unwrap();
            let cidx = registers.cid_register.register(&cid);
            let prop = get_op_prop(op, &mut registers);
            let value_kind = encode_op(
                op,
                arena,
                &mut del_starts,
                &mut value_writer,
                &mut registers,
            );
            encoded_ops.push(EncodedOp {
                container_index: cidx as u32,
                prop,
                value_type: value_kind.to_u8(),
                len: op.atom_len() as u32,
            })
        }
    }

    let cids = registers.cid_register.unwrap_vec();
    let container_arena = ContainerArena::from_containers(
        cids,
        &mut registers.peer_register,
        &mut registers.key_register,
    );

    // Write to output

    // PeerIDs
    let peers = registers.peer_register.unwrap_vec();
    let peer_bytes: Vec<u8> = peers.iter().flat_map(|p| p.to_le_bytes()).collect();

    // First Counter + Change Len
    let mut lengths_bytes = Vec::new();
    for c in block {
        leb128::write::unsigned(&mut lengths_bytes, c.atom_len() as u64).unwrap();
    }

    //      ┌────────────────────┬─────────────────────────────────────────┐
    //      │  Key Strings Size  │               Key Strings               │
    //      └────────────────────┴─────────────────────────────────────────┘
    let keys = registers.key_register.unwrap_vec();
    let keys_bytes = encode_keys(keys);

    //      ┌────────────────────┬─────────────────────────────────────────┐
    //      │  Position Size     │               Position                  │
    //      └────────────────────┴─────────────────────────────────────────┘
    let position_vec = registers.position_register.unwrap_vec();
    let positions = PositionArena::from_positions(position_vec.iter().map(|p| p.as_bytes()));
    let position_bytes = positions.encode();

    //      ┌──────────┬──────────┬───────┬────────────────────────────────┐
    //      │          │          │       │                                │
    //      │          │          │       │                                │
    //      │          │          │       │                                │
    //      │  LEB128  │   RLE    │ Delta │                                │
    //      │ Lengths  │Containers│  RLE  │           ValueType            │
    //      │          │          │ Props │                                │
    //      │          │          │       │                                │
    //      │          │          │       │                                │
    //      │          │          │       │                                │
    //      └──────────┴──────────┴───────┴────────────────────────────────┘

    let ops_bytes = serde_columnar::to_vec(&EncodedOpsAndDeleteStarts {
        ops: encoded_ops,
        delete_start_ids: del_starts,
    })
    .unwrap();
    //      ┌────────────────┬─────────────────────────────────────────────┐
    //      │Value Bytes Size│                Value Bytes                  │
    //      └────────────────┴─────────────────────────────────────────────┘

    let value_bytes = value_writer.finish();
    let out = EncodedBlock {
        version: VERSION,
        counter_start: block[0].id.counter as u32,
        counter_len: (block.last().unwrap().ctr_end() - block[0].id.counter) as u32,
        lamport_start: block[0].lamport(),
        lamport_len: block.last().unwrap().lamport_end() - block[0].lamport(),
        n_changes: block.len() as u32,
        first_counter: block[0].id.counter as u32,
        peers: Cow::Owned(peer_bytes),
        lengths: Cow::Owned(lengths_bytes),
        dep_on_self: dep_self_encoder.finish().unwrap().into(),
        dep_len: dep_len_encoder.finish().unwrap().into(),
        dep_peer_idxs: encoded_deps.peer_idx.finish().unwrap().into(),
        dep_counters: encoded_deps.counter.finish().unwrap().into(),
        lamports: lamport_encoder.finish().0.into(),
        timestamps: timestamp_encoder.finish().unwrap().into(),
        commit_msg_lengths: commit_msg_len_encoder.finish().unwrap().into(),
        commit_msgs: Cow::Owned(commit_msgs.into_bytes()),
        cids: container_arena.encode().into(),
        keys: keys_bytes.into(),
        positions: position_bytes.into(),
        ops: ops_bytes.into(),
        values: value_bytes.into(),
    };
    postcard::to_allocvec(&out).unwrap()
}

fn encode_keys(keys: Vec<loro_common::InternalString>) -> Vec<u8> {
    let mut keys_bytes = Vec::new();
    for key in keys {
        let bytes = key.as_bytes();
        leb128::write::unsigned(&mut keys_bytes, bytes.len() as u64).unwrap();
        keys_bytes.write_all(bytes).unwrap();
    }
    keys_bytes
}

fn decode_keys(mut bytes: &[u8]) -> Vec<InternalString> {
    let mut keys = Vec::new();
    while !bytes.is_empty() {
        let len = leb128::read::unsigned(&mut bytes).unwrap();
        let key = std::str::from_utf8(&bytes[..len as usize]).unwrap();
        keys.push(key.into());
        bytes = &bytes[len as usize..];
    }

    keys
}

struct Registers {
    peer_register: ValueRegister<PeerID>,
    key_register: ValueRegister<loro_common::InternalString>,
    cid_register: ValueRegister<ContainerID>,
    position_register: ValueRegister<FractionalIndex>,
}

use crate::encoding::value::{
    RawTreeMove, Value, ValueDecodedArenasTrait, ValueEncodeRegister, ValueKind, ValueReader,
    ValueWriter,
};
use crate::version::Frontiers;
impl ValueEncodeRegister for Registers {
    fn key_mut(&mut self) -> &mut ValueRegister<loro_common::InternalString> {
        &mut self.key_register
    }

    fn peer_mut(&mut self) -> &mut ValueRegister<PeerID> {
        &mut self.peer_register
    }

    fn encode_tree_op(&mut self, op: &tree_op::TreeOp) -> encoding::value::Value<'static> {
        match op {
            tree_op::TreeOp::Create {
                target,
                parent,
                position,
            } => encoding::value::Value::RawTreeMove(RawTreeMove {
                subject_peer_idx: self.peer_register.register(&target.peer),
                subject_cnt: target.counter,
                is_parent_null: parent.is_none(),
                parent_peer_idx: parent.map_or(0, |p| self.peer_register.register(&p.peer)),
                parent_cnt: parent.map_or(0, |p| p.counter),
                position_idx: self.position_register.register(position),
            }),
            tree_op::TreeOp::Move {
                target,
                parent,
                position,
            } => encoding::value::Value::RawTreeMove(RawTreeMove {
                subject_peer_idx: self.peer_register.register(&target.peer),
                subject_cnt: target.counter,
                is_parent_null: parent.is_none(),
                parent_peer_idx: parent.map_or(0, |p| self.peer_register.register(&p.peer)),
                parent_cnt: parent.map_or(0, |p| p.counter),
                position_idx: self.position_register.register(position),
            }),
            tree_op::TreeOp::Delete { target } => {
                let parent = TreeID::delete_root();
                encoding::value::Value::RawTreeMove(RawTreeMove {
                    subject_peer_idx: self.peer_register.register(&target.peer),
                    subject_cnt: target.counter,
                    is_parent_null: false,
                    parent_peer_idx: self.peer_register.register(&parent.peer),
                    parent_cnt: parent.counter,
                    position_idx: 0,
                })
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ChangesBlockHeader {
    pub peer: PeerID,
    pub counter: Counter,
    pub n_changes: usize,
    pub peers: Vec<PeerID>,
    /// This has n + 1 elements, where counters[n] is the end counter of the
    /// last change in the block.
    ///
    /// You can infer the size of kth change by taking counters[k + 1] - counters[k]
    pub counters: Vec<Counter>,
    /// This has n elements
    pub lamports: Vec<Lamport>,
    pub deps_groups: Vec<Frontiers>,
    pub keys: OnceCell<Vec<InternalString>>,
    pub cids: OnceCell<Vec<ContainerID>>,
}

pub fn decode_header(m_bytes: &[u8]) -> LoroResult<ChangesBlockHeader> {
    let doc = postcard::from_bytes(m_bytes).map_err(|e| {
        LoroError::DecodeError(format!("Decode block error {}", e).into_boxed_str())
    })?;

    decode_header_from_doc(&doc)
}

// MARK: decode_block_from_doc
fn decode_header_from_doc(doc: &EncodedBlock) -> Result<ChangesBlockHeader, LoroError> {
    let EncodedBlock {
        n_changes,
        first_counter,
        peers: peers_bytes,
        lengths: lengths_bytes,
        dep_on_self,
        dep_len,
        dep_peer_idxs,
        dep_counters,
        lamports,
        version,
        counter_len,
        counter_start,
        lamport_len,
        lamport_start,
        ..
    } = doc;

    if *version != VERSION {
        return Err(LoroError::IncompatibleFutureEncodingError(
            *version as usize,
        ));
    }

    let first_counter = *first_counter as Counter;
    let n_changes = *n_changes as usize;
    let peer_num = peers_bytes.len() / 8;
    let mut peers = Vec::with_capacity(peer_num);
    for i in 0..peer_num {
        let peer_id =
            PeerID::from_le_bytes((&peers_bytes[(8 * i)..(8 * (i + 1))]).try_into().unwrap());
        peers.push(peer_id);
    }

    // ┌───────────────────┬──────────────────────────────────────────┐    │
    // │ LEB First Counter │         N LEB128 Change AtomLen          │◁───┼─────  Important metadata
    // └───────────────────┴──────────────────────────────────────────┘    │
    let mut lengths = Vec::with_capacity(n_changes);
    let mut lengths_bytes: &[u8] = lengths_bytes;
    for _ in 0..n_changes {
        lengths.push(leb128::read::unsigned(&mut lengths_bytes).unwrap() as Counter);
    }

    // ┌───────────────────┬────────────────────────┬─────────────────┐    │
    // │N DepOnSelf BoolRle│ N Delta Rle Deps Lens  │    N Dep IDs    │◁───┘
    // └───────────────────┴────────────────────────┴─────────────────┘

    let mut dep_self_decoder = BoolRleDecoder::new(dep_on_self);
    let mut this_counter = first_counter;
    let mut deps: Vec<Frontiers> = Vec::with_capacity(n_changes);
    let n = n_changes;
    let mut deps_len = AnyRleDecoder::<u64>::new(dep_len);
    let deps_peers_decoder = AnyRleDecoder::<u32>::new(dep_peer_idxs);
    let deps_counters_decoder = AnyRleDecoder::<u32>::new(dep_counters);
    let mut deps_peers_iter = deps_peers_decoder;
    let mut deps_counters_iter = deps_counters_decoder;
    for i in 0..n {
        let mut f = Frontiers::default();

        if dep_self_decoder.next().unwrap().unwrap() {
            f.push(ID::new(peers[0], this_counter - 1))
        }

        let len = deps_len.next().unwrap().unwrap() as usize;
        for _ in 0..len {
            let peer_idx = deps_peers_iter.next().unwrap().unwrap() as usize;
            let peer = peers[peer_idx];
            let counter = deps_counters_iter.next().unwrap().unwrap() as Counter;
            f.push(ID::new(peer, counter));
        }

        deps.push(f);
        this_counter += lengths[i];
    }

    let mut counters = Vec::with_capacity(n + 1);
    let mut last = first_counter;
    for i in 0..n {
        counters.push(last);
        last += lengths[i];
    }
    counters.push(last);
    assert_eq!(last, (counter_start + counter_len) as Counter);
    let mut lamport_decoder = UnsignedDeltaDecoder::new(lamports, n_changes);
    let mut lamports = Vec::with_capacity(n + 1);
    for _ in 0..n {
        lamports.push(lamport_decoder.next().unwrap() as Lamport);
    }

    let last_lamport = *lamports.last().unwrap();
    lamports.push(last_lamport + lengths.last().copied().unwrap() as Lamport);
    assert_eq!(
        *lamports.last().unwrap(),
        (lamport_start + lamport_len) as Lamport
    );
    Ok(ChangesBlockHeader {
        peer: peers[0],
        counter: first_counter,
        n_changes,
        peers,
        counters,
        deps_groups: deps,
        lamports,
        keys: OnceCell::new(),
        cids: OnceCell::new(),
    })
}

struct EncodedDeps {
    peer_idx: AnyRleEncoder<u32>,
    counter: AnyRleEncoder<u32>,
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
struct EncodedOp {
    #[columnar(strategy = "DeltaRle")]
    container_index: u32,
    #[columnar(strategy = "DeltaRle")]
    prop: i32,
    #[columnar(strategy = "DeltaRle")]
    value_type: u8,
    #[columnar(strategy = "Rle")]
    len: u32,
}

#[columnar(ser, de)]
struct EncodedOpsAndDeleteStarts {
    #[columnar(class = "vec", iter = "EncodedOp")]
    ops: Vec<EncodedOp>,
    #[columnar(class = "vec", iter = "EncodedDeleteStartId")]
    delete_start_ids: Vec<EncodedDeleteStartId>,
}

struct ValueDecodeArena<'a> {
    peers: &'a [PeerID],
    keys: &'a [InternalString],
}

impl<'a> ValueDecodedArenasTrait for ValueDecodeArena<'a> {
    fn keys(&self) -> &[InternalString] {
        self.keys
    }

    fn peers(&self) -> &[PeerID] {
        self.peers
    }

    fn decode_tree_op(
        &self,
        _positions: &[Vec<u8>],
        _op: encoding::value::EncodedTreeMove,
        _id: ID,
    ) -> LoroResult<tree_op::TreeOp> {
        unreachable!()
    }
}

pub fn decode_block_range(
    mut bytes: &[u8],
) -> LoroResult<((Counter, Counter), (Lamport, Lamport))> {
    let version = leb128::read::unsigned(&mut bytes).map_err(|e| {
        LoroError::DecodeError(format!("Failed to read version: {}", e).into_boxed_str())
    })?;

    if version as u16 != VERSION {
        return Err(LoroError::DecodeError(
            "Version mismatch".to_string().into_boxed_str(),
        ));
    }

    let counter_start = leb128::read::unsigned(&mut bytes).unwrap() as Counter;
    let counter_len = leb128::read::unsigned(&mut bytes).unwrap() as Counter;
    let lamport_start = leb128::read::unsigned(&mut bytes).unwrap() as Lamport;
    let lamport_len = leb128::read::unsigned(&mut bytes).unwrap() as Lamport;
    Ok((
        (counter_start, counter_start + counter_len),
        (lamport_start, lamport_start + lamport_len),
    ))
}

/// Ensure the cids in header are decoded
#[allow(unused)]
pub fn decode_cids(
    bytes: &[u8],
    header: Option<ChangesBlockHeader>,
) -> LoroResult<ChangesBlockHeader> {
    let doc = postcard::from_bytes(bytes).map_err(|e| {
        LoroError::DecodeError(format!("Decode block error {}", e).into_boxed_str())
    })?;
    let header = if let Some(h) = header {
        h
    } else {
        let doc = postcard::from_bytes(bytes).map_err(|e| {
            LoroError::DecodeError(format!("Decode block error {}", e).into_boxed_str())
        })?;
        decode_header_from_doc(&doc)?
    };

    if header.cids.get().is_some() {
        return Ok(header);
    }

    let EncodedBlock { cids, keys, .. } = doc;
    let keys = header.keys.get_or_init(|| decode_keys(&keys));
    let decode_arena = ValueDecodeArena {
        peers: &header.peers,
        keys,
    };

    header
        .cids
        .set(
            ContainerArena::decode(&cids)
                .unwrap()
                .iter()
                .map(|x| x.as_container_id(&decode_arena))
                .try_collect()
                .unwrap(),
        )
        .unwrap();
    Ok(header)
}

// MARK: decode_block
pub fn decode_block(
    m_bytes: &[u8],
    shared_arena: &SharedArena,
    header: Option<&ChangesBlockHeader>,
) -> LoroResult<Vec<Change>> {
    let doc = postcard::from_bytes(m_bytes).map_err(|e| {
        LoroError::DecodeError(format!("Decode block error {}", e).into_boxed_str())
    })?;
    let mut header_on_stack = None;
    let header = header.unwrap_or_else(|| {
        header_on_stack = Some(decode_header_from_doc(&doc).unwrap());
        header_on_stack.as_ref().unwrap()
    });
    let EncodedBlock {
        version,
        n_changes,
        first_counter,
        timestamps,
        commit_msg_lengths,
        commit_msgs,
        cids,
        keys,
        ops,
        values,
        positions,
        ..
    } = doc;
    let mut changes = Vec::with_capacity(n_changes as usize);
    if version != VERSION {
        return Err(LoroError::IncompatibleFutureEncodingError(version as usize));
    }
    let mut timestamp_decoder: DeltaRleDecoder<i64> = DeltaRleDecoder::new(&timestamps);
    let mut commit_msg_len_decoder = AnyRleDecoder::<u32>::new(&commit_msg_lengths);
    let mut commit_msg_index = 0;
    let keys = header.keys.get_or_init(|| decode_keys(&keys));
    let decode_arena = ValueDecodeArena {
        peers: &header.peers,
        keys,
    };
    let positions = PositionArena::decode(&positions)?;
    let positions = positions.parse_to_positions();
    let cids: &Vec<ContainerID> = header.cids.get_or_init(|| {
        ContainerArena::decode(&cids)
            .unwrap()
            .iter()
            .map(|x| x.as_container_id(&decode_arena))
            .try_collect()
            .unwrap()
    });
    let mut value_reader = ValueReader::new(&values);
    let encoded_ops_iters =
        serde_columnar::iter_from_bytes::<EncodedOpsAndDeleteStarts>(&ops).unwrap();
    let op_iter = encoded_ops_iters.ops;
    let mut del_iter = encoded_ops_iters.delete_start_ids;
    for i in 0..(n_changes as usize) {
        let commit_msg: Option<Arc<str>> = {
            let len = commit_msg_len_decoder.next().unwrap().unwrap();
            if len == 0 {
                None
            } else {
                let end = commit_msg_index + len;
                match std::str::from_utf8(&commit_msgs[commit_msg_index as usize..end as usize]) {
                    Ok(s) => {
                        commit_msg_index = end;
                        Some(Arc::from(s))
                    }
                    Err(_) => {
                        tracing::error!("Invalid UTF8 String");
                        return LoroResult::Err(LoroError::DecodeDataCorruptionError);
                    }
                }
            }
        };
        changes.push(Change {
            ops: Default::default(),
            deps: header.deps_groups[i].clone(),
            id: ID::new(header.peer, header.counters[i]),
            lamport: header.lamports[i],
            timestamp: timestamp_decoder.next().unwrap().unwrap() as Timestamp,
            commit_msg,
        })
    }

    let mut counter = first_counter as Counter;
    let mut change_index = 0;
    let peer = header.peer;
    for op in op_iter {
        let EncodedOp {
            container_index,
            prop,
            value_type,
            len,
        } = op?;
        let value = Value::decode(
            ValueKind::from_u8(value_type),
            &mut value_reader,
            &decode_arena,
            ID::new(peer, counter),
        )?;

        let cid = &cids[container_index as usize];
        let content = decode_op(
            cid,
            value,
            &mut del_iter,
            shared_arena,
            &decode_arena,
            &positions,
            prop,
            ID::new(peer, counter),
        )?;

        let c_idx = shared_arena.register_container(cid);
        let op = Op {
            counter,
            container: c_idx,
            content,
        };

        changes[change_index].ops.push(op);
        counter += len as Counter;
        if counter >= header.counters[change_index + 1] {
            change_index += 1;
        }
    }

    Ok(changes)
}
