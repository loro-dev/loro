//! # Encode
//!
//!      ≈4KB after compression
//!
//!       N = Number of Changes
//!
//!       Peer_1 = This Peer
//!
//!
//!      ┌──────────┬─────┬────────────┬───────────────────────────────┐
//!      │2B Version│LEB N│LEB Peer Num│     8B Peer_1,...,Peer_x      │◁────┐
//!      └──────────┴─────┴────────────┴───────────────────────────────┘     │
//!      ┌──────────────────────────────────────────────────────────────┐    │
//!      │                   N LEB128 Delta Counters                    │◁───┼─────  Important metadata
//!      └──────────────────────────────────────────────────────────────┘    │
//!      ┌───────────────────┬────────────────────────┬─────────────────┐    │
//!      │N DepOnSelf BoolRle│ N Delta Rle Deps Lens  │    N Dep IDs    │◁───┘
//!      └───────────────────┴────────────────────────┴─────────────────┘
//!      ┌──────────────────────────────────────────────────────────────┐
//!      │                   N LEB128 Delta Lamports                    │
//!      └──────────────────────────────────────────────────────────────┘
//!      ┌──────────────────────────────────────────────────────────────┐
//!      │                  N LEB128 Delta Timestamps                   │
//!      └──────────────────────────────────────────────────────────────┘
//!      ┌────────────────────────────────┬─────────────────────────────┐
//!      │    N Rle Commit Msg Lengths    │       Commit Messages       │
//!      └────────────────────────────────┴─────────────────────────────┘
//!
//!       ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ Encoded Operations ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─
//!
//!      ┌────────────────────┬─────────────────────────────────────────┐
//!      │ ContainerIDs Size  │             ContainerIDs                │
//!      └────────────────────┴─────────────────────────────────────────┘
//!      ┌────────────────────┬─────────────────────────────────────────┐
//!      │  Key Strings Size  │               Key Strings               │
//!      └────────────────────┴─────────────────────────────────────────┘
//!      ┌──────────┬──────────┬───────┬────────────────────────────────┐
//!      │          │          │       │                                │
//!      │          │          │       │                                │
//!      │          │          │       │                                │
//!      │  LEB128  │   RLE    │ Delta │                                │
//!      │ Lengths  │Containers│  RLE  │           ValueType            │
//!      │          │          │ Props │                                │
//!      │          │          │       │                                │
//!      │          │          │       │                                │
//!      │          │          │       │                                │
//!      └──────────┴──────────┴───────┴────────────────────────────────┘
//!      ┌────────────────┬─────────────────────────────────────────────┐
//!      │Value Bytes Size│                Value Bytes                  │
//!      └────────────────┴─────────────────────────────────────────────┘

use std::io::Write;

use loro_common::{ContainerID, PeerID};
use rle::HasLength;
use serde_columnar::columnar;

use super::delta_rle_encode::{BoolRleEncoder, UnsignedDeltaEncoder, UnsignedRleEncoder};
use super::ChangesBlock;
use crate::arena::SharedArena;
use crate::change::Change;
use crate::encoding::arena::ContainerArena;
use crate::encoding::value_register::ValueRegister;
use crate::encoding::{encode_op, get_op_prop};

const VERSION: u16 = 0;

/// It's assume that every change in the block share the same peer.
pub fn encode(block: &[Change], arena: &SharedArena) -> Vec<u8> {
    if block.is_empty() {
        panic!("Empty block")
    }

    let mut output = Vec::with_capacity(4096);
    output.write_all(&VERSION.to_le_bytes()).unwrap();
    leb128::write::unsigned(&mut output, block.len() as u64).unwrap();
    let mut peer_register: ValueRegister<PeerID> = ValueRegister::new();
    let peer = block[0].peer();
    peer_register.register(&peer);

    let cid_register: ValueRegister<ContainerID> = ValueRegister::new();
    let mut counter_encoder = UnsignedDeltaEncoder::new(block.len() * 2 + 4);
    let mut timestamp_encoder = UnsignedDeltaEncoder::new(block.len() * 3 + 8);
    let mut lamport_encoder = UnsignedDeltaEncoder::new(block.len() * 2 + 4);
    let mut commit_msg_len_encoder = UnsignedRleEncoder::new(0);
    let mut dep_self_encoder = BoolRleEncoder::new();
    let mut dep_len_encoder = UnsignedRleEncoder::new(0);
    let mut encoded_deps = EncodedDeps {
        peer_idx: UnsignedRleEncoder::new(0),
        counter: UnsignedRleEncoder::new(0),
    };
    for c in block {
        counter_encoder.push(c.id.counter as u64);
        timestamp_encoder.push(c.timestamp() as u64);
        lamport_encoder.push(c.lamport() as u64);
        commit_msg_len_encoder.push(0);

        let mut dep_on_self = false;
        for dep in c.deps().iter() {
            if dep.peer == peer {
                dep_on_self = true;
            } else {
                let peer_idx = peer_register.register(&dep.peer);
                encoded_deps.peer_idx.push(peer_idx as u64);
                encoded_deps.counter.push(dep.counter as u64);
            }
        }

        dep_self_encoder.push(dep_on_self);
        dep_len_encoder.push(if dep_on_self {
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

    let mut del_starts = Vec::new();
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
    leb128::write::unsigned(&mut output, peers.len() as u64).unwrap();
    for peer in peers {
        output.write_all(&peer.to_le_bytes()).unwrap();
    }

    // Counters
    let (bytes, _n) = counter_encoder.finish();
    output.write_all(&bytes).unwrap();

    //      ┌───────────────────┬────────────────────────┬─────────────────┐
    //      │N DepOnSelf BoolRle│ N Delta Rle Deps Lens  │    N Dep IDs    │
    //      └───────────────────┴────────────────────────┴─────────────────┘
    let (buf, _n) = dep_self_encoder.finish();
    output.write_all(&buf).unwrap();
    let (buf, _n) = dep_len_encoder.finish();
    output.write_all(&buf).unwrap();
    let (buf, _n) = encoded_deps.peer_idx.finish();
    output.write_all(&buf).unwrap();
    let (buf, _n) = encoded_deps.counter.finish();
    output.write_all(&buf).unwrap();

    //      ┌──────────────────────────────────────────────────────────────┐
    //      │                   N LEB128 Delta Lamports                    │
    //      └──────────────────────────────────────────────────────────────┘
    let (buf, _n) = lamport_encoder.finish();
    output.write_all(&buf).unwrap();

    //      ┌──────────────────────────────────────────────────────────────┐
    //      │                  N LEB128 Delta Timestamps                   │
    //      └──────────────────────────────────────────────────────────────┘
    let (buf, _n) = timestamp_encoder.finish();
    output.write_all(&buf).unwrap();

    //      ┌────────────────────────────────┬─────────────────────────────┐
    //      │    N Rle Commit Msg Lengths    │       Commit Messages       │
    //      └────────────────────────────────┴─────────────────────────────┘
    let (buf, _n) = commit_msg_len_encoder.finish();
    output.write_all(&buf).unwrap();
    // TODO: Commit messages

    //       ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ Encoded Operations ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─
    //
    //      ┌────────────────────┬─────────────────────────────────────────┐
    //      │ ContainerIDs Size  │             ContainerIDs                │
    //      └────────────────────┴─────────────────────────────────────────┘

    let bytes = container_arena.encode();
    leb128::write::unsigned(&mut output, bytes.len() as u64).unwrap();
    output.write_all(&bytes).unwrap();

    //      ┌────────────────────┬─────────────────────────────────────────┐
    //      │  Key Strings Size  │               Key Strings               │
    //      └────────────────────┴─────────────────────────────────────────┘
    let keys = registers.key_register.unwrap_vec();
    leb128::write::unsigned(&mut output, keys.len() as u64).unwrap();
    for key in keys {
        let bytes = key.as_bytes();
        leb128::write::unsigned(&mut output, bytes.len() as u64).unwrap();
        output.write_all(bytes).unwrap();
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
    leb128::write::unsigned(&mut output, ops_bytes.len() as u64).unwrap();
    output.write_all(&ops_bytes).unwrap();
    //      ┌────────────────┬─────────────────────────────────────────────┐
    //      │Value Bytes Size│                Value Bytes                  │
    //      └────────────────┴─────────────────────────────────────────────┘

    let value_bytes = value_writer.finish();
    leb128::write::unsigned(&mut output, value_bytes.len() as u64).unwrap();
    output.write_all(&value_bytes).unwrap();
    output
}

struct Registers {
    peer_register: ValueRegister<PeerID>,
    key_register: ValueRegister<loro_common::InternalString>,
    cid_register: ValueRegister<ContainerID>,
}

use crate::encoding::value::{ValueEncodeRegister, ValueWriter};
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

struct EncodedDeps {
    peer_idx: UnsignedRleEncoder,
    counter: UnsignedRleEncoder,
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
