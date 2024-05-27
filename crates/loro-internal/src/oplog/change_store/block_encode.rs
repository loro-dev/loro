//! # Encode
//!
//!
//!    ≈4KB after compression
//!
//!     N = Number of Changes
//!
//!     Peer_1 = This Peer
//!
//!
//!    ┌──────────┬─────┬────────────┬───────────────────────────────┐
//!    │2B Version│LEB N│LEB Peer Num│     8B Peer_1,...,Peer_x      │◁────┐
//!    └──────────┴─────┴────────────┴───────────────────────────────┘     │
//!    ┌───────────────────┬──────────────────────────────────────────┐    │
//!    │ LEB First Counter │         N LEB128 Change AtomLen          │◁───┼─────  Important metadata
//!    └───────────────────┴──────────────────────────────────────────┘    │
//!    ┌───────────────────┬────────────────────────┬─────────────────┐    │
//!    │N DepOnSelf BoolRle│ N Delta Rle Deps Lens  │     Dep IDs     │◁───┤
//!    └───────────────────┴────────────────────────┴─────────────────┘    │
//!    ┌──────────────────────────────────────────────────────────────┐    │
//!    │                   N LEB128 Delta Lamports                    │◁───┘
//!    └──────────────────────────────────────────────────────────────┘
//!    ┌──────────────────────────────────────────────────────────────┐
//!    │                  N LEB128 Delta Timestamps                   │
//!    └──────────────────────────────────────────────────────────────┘
//!    ┌────────────────────────────────┬─────────────────────────────┐
//!    │    N Rle Commit Msg Lengths    │       Commit Messages       │
//!    └────────────────────────────────┴─────────────────────────────┘
//!
//!     ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ Encoded Operations ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─
//!
//!    ┌────────────────────┬─────────────────────────────────────────┐
//!    │ ContainerIDs Size  │             ContainerIDs                │
//!    └────────────────────┴─────────────────────────────────────────┘
//!    ┌────────────────────┬─────────────────────────────────────────┐
//!    │  Key Strings Size  │               Key Strings               │
//!    └────────────────────┴─────────────────────────────────────────┘
//!    ┌────────┬──────────┬──────────┬───────┬───────────────────────┐
//!    │        │          │          │       │                       │
//!    │        │          │          │       │                       │
//!    │        │          │          │       │                       │
//!    │  Ops   │  LEB128  │   RLE    │ Delta │                       │
//!    │  Size  │ Lengths  │Containers│  RLE  │       ValueType       │
//!    │        │          │          │ Props │                       │
//!    │        │          │          │       │                       │
//!    │        │          │          │       │                       │
//!    │        │          │          │       │                       │
//!    └────────┴──────────┴──────────┴───────┴───────────────────────┘
//!    ┌────────────────┬─────────────────────────────────────────────┐
//!    │                │                                             │
//!    │Value Bytes Size│                Value Bytes                  │
//!    │                │                                             │
//!    └────────────────┴─────────────────────────────────────────────┘
//!    ┌──────────────────────────────────────────────────────────────┐
//!    │                                                              │
//!    │                       Delete Start IDs                       │
//!    │                                                              │
//!    └──────────────────────────────────────────────────────────────┘

use std::borrow::Cow;
use std::io::Write;

use loro_common::{ContainerID, Counter, LoroError, LoroResult, PeerID, ID};
use num::complex::ParseComplexError;
use rle::HasLength;
use serde::{Deserialize, Serialize};
use serde_columnar::{columnar, Itertools};

use super::delta_rle_encode::UnsignedDeltaEncoder;
use crate::arena::SharedArena;
use crate::change::Change;
use crate::encoding::arena::ContainerArena;
use crate::encoding::value_register::ValueRegister;
use crate::encoding::{encode_op, get_op_prop};
use serde_columnar::{
    AnyRleDecoder, AnyRleEncoder, BoolRleDecoder, BoolRleEncoder, DeltaRleEncoder,
};

const VERSION: u16 = 0;

/// It's assume that every change in the block share the same peer.
pub fn encode(block: &[Change], arena: &SharedArena) -> Vec<u8> {
    if block.is_empty() {
        panic!("Empty block")
    }

    let mut peer_register: ValueRegister<PeerID> = ValueRegister::new();
    let peer = block[0].peer();
    peer_register.register(&peer);

    let cid_register: ValueRegister<ContainerID> = ValueRegister::new();
    let mut timestamp_encoder = UnsignedDeltaEncoder::new(block.len() * 3 + 8);
    let mut lamport_encoder = UnsignedDeltaEncoder::new(block.len() * 2 + 4);
    let mut commit_msg_len_encoder = AnyRleEncoder::<u32>::new();
    let mut dep_self_encoder = BoolRleEncoder::new();
    let mut dep_len_encoder = AnyRleEncoder::<u64>::new();
    let mut encoded_deps = EncodedDeps {
        peer_idx: AnyRleEncoder::new(),
        counter: AnyRleEncoder::new(),
    };
    for c in block {
        timestamp_encoder.push(c.timestamp() as u64);
        lamport_encoder.push(c.lamport() as u64);
        commit_msg_len_encoder.append(0);

        let mut dep_on_self = false;
        for dep in c.deps().iter() {
            if dep.peer == peer {
                dep_on_self = true;
            } else {
                let peer_idx = peer_register.register(&dep.peer);
                encoded_deps.peer_idx.append(peer_idx as u32);
                encoded_deps.counter.append(dep.counter as u32);
            }
        }

        dep_self_encoder.append(dep_on_self);
        dep_len_encoder.append(if dep_on_self {
            c.deps().len() as u64 - 1
        } else {
            c.deps().len() as u64
        });
    }

    let mut encoded_ops = Vec::new();
    let mut registers = Registers {
        peer_register,
        key_register: ValueRegister::new(),
        cid_register,
    };

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
    let peer_bytes: Vec<u8> = peers.iter().map(|p| p.to_le_bytes()).flatten().collect();

    // Frist Counter + Change Len
    let mut lengths_bytes = Vec::new();
    for c in block {
        leb128::write::unsigned(&mut lengths_bytes, c.atom_len() as u64).unwrap();
    }

    //      ┌────────────────────┬─────────────────────────────────────────┐
    //      │  Key Strings Size  │               Key Strings               │
    //      └────────────────────┴─────────────────────────────────────────┘
    let keys = registers.key_register.unwrap_vec();
    let mut keys_bytes = Vec::new();
    for key in keys {
        let bytes = key.as_bytes();
        leb128::write::unsigned(&mut keys_bytes, bytes.len() as u64).unwrap();
        keys_bytes.write_all(bytes).unwrap();
    }

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

    let ops_bytes = serde_columnar::to_vec(&encoded_ops).unwrap();
    //      ┌────────────────┬─────────────────────────────────────────────┐
    //      │Value Bytes Size│                Value Bytes                  │
    //      └────────────────┴─────────────────────────────────────────────┘

    let value_bytes = value_writer.finish();
    let out = EncodedDoc {
        version: VERSION,
        n_changes: block.len() as u32,
        first_counter: block[0].id.counter as u32,
        peers: Cow::Owned(peer_bytes),
        lengths: Cow::Owned(lengths_bytes),
        dep_on_self: dep_self_encoder.finish().unwrap().into(),
        dep_len: dep_len_encoder.finish().unwrap().into(),
        dep_peer_idxs: encoded_deps.peer_idx.finish().unwrap().into(),
        dep_counters: encoded_deps.counter.finish().unwrap().into(),
        lamports: lamport_encoder.finish().0.into(),
        timestamps: timestamp_encoder.finish().0.into(),
        commit_msg_lengths: commit_msg_len_encoder.finish().unwrap().into(),
        commit_msgs: Cow::Owned(vec![]),
        cids: container_arena.encode().into(),
        keys: keys_bytes.into(),
        ops: ops_bytes.into(),
        values: value_bytes.into(),
        delete_start_ids: serde_columnar::to_vec(&del_starts).unwrap().into(),
    };
    postcard::to_allocvec(&out).unwrap()
}

struct Registers {
    peer_register: ValueRegister<PeerID>,
    key_register: ValueRegister<loro_common::InternalString>,
    cid_register: ValueRegister<ContainerID>,
}

use crate::encoding::value::{ValueEncodeRegister, ValueWriter};
use crate::version::Frontiers;
impl ValueEncodeRegister for Registers {
    fn key_mut(&mut self) -> &mut ValueRegister<loro_common::InternalString> {
        &mut self.key_register
    }

    fn peer_mut(&mut self) -> &mut ValueRegister<PeerID> {
        &mut self.peer_register
    }

    fn encode_tree_op(
        &mut self,
        op: &crate::container::tree::tree_op::TreeOp,
    ) -> crate::encoding::value::Value<'static> {
        todo!()
    }
}

pub(crate) struct BlockHeader {
    peer: PeerID,
    counter: Counter,
    n_changes: usize,
    peers: Vec<PeerID>,
    /// This has n + 1 elements, where counters[n] is the end counter of the
    /// last change in the block.
    counters: Vec<Counter>,
    deps: Vec<Frontiers>,
}

pub fn decode_header(m_bytes: &[u8]) -> LoroResult<BlockHeader> {
    let EncodedDoc {
        version,
        n_changes,
        first_counter,
        peers: peers_bytes,
        lengths: lengths_bytes,
        dep_on_self,
        dep_len,
        dep_peer_idxs,
        dep_counters,
        lamports,
        timestamps,
        commit_msg_lengths,
        commit_msgs,
        cids,
        keys,
        ops,
        values,
        delete_start_ids,
    } = postcard::from_bytes(m_bytes).map_err(|e| {
        LoroError::DecodeError(format!("Decode block error {}", e).into_boxed_str())
    })?;
    if version != VERSION {
        return Err(LoroError::IncompatibleFutureEncodingError(version as usize));
    }

    let first_counter = first_counter as Counter;
    let n_changes = n_changes as usize;
    let peer_num = peers_bytes.len() / 8;
    let mut peers = Vec::with_capacity(peer_num as usize);
    for i in 0..(n_changes as usize) {
        let peer_id =
            PeerID::from_le_bytes((&peers_bytes[(8 * i)..(8 * (i + 1))]).try_into().unwrap());
        peers.push(peer_id);
    }

    // ┌───────────────────┬──────────────────────────────────────────┐    │
    // │ LEB First Counter │         N LEB128 Change AtomLen          │◁───┼─────  Important metadata
    // └───────────────────┴──────────────────────────────────────────┘    │
    let mut lengths = Vec::with_capacity(n_changes as usize);
    let mut lengths_bytes: &[u8] = &*lengths_bytes;
    for _ in 0..n_changes {
        lengths.push(leb128::read::unsigned(&mut lengths_bytes).unwrap() as Counter);
    }

    // ┌───────────────────┬────────────────────────┬─────────────────┐    │
    // │N DepOnSelf BoolRle│ N Delta Rle Deps Lens  │    N Dep IDs    │◁───┘
    // └───────────────────┴────────────────────────┴─────────────────┘

    let mut dep_self_decoder = BoolRleDecoder::new(&dep_on_self);
    let mut this_counter = first_counter;
    let deps: Vec<Frontiers> = Vec::with_capacity(n_changes);
    let n = n_changes;
    let mut deps_len = AnyRleDecoder::<u64>::new(&dep_len);
    let deps_peers_decoder = AnyRleDecoder::<u32>::new(&dep_peer_idxs);
    let deps_counters_decoder = AnyRleDecoder::<u32>::new(&dep_counters);
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

        this_counter += lengths[i];
    }

    let mut counters = Vec::with_capacity(n + 1);
    let mut last = first_counter;
    for i in 0..n {
        counters.push(last);
        last += lengths[i];
    }
    counters.push(last);
    Ok(BlockHeader {
        peer: peers[0],
        counter: first_counter,
        n_changes,
        peers,
        counters,
        deps,
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

#[derive(Serialize, Deserialize)]
struct EncodedDoc<'a> {
    version: u16,
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
    ops: Cow<'a, [u8]>,
    #[serde(borrow)]
    values: Cow<'a, [u8]>,
    #[serde(borrow)]
    delete_start_ids: Cow<'a, [u8]>,
}
