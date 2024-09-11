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
//! ┌────────────────────────┬─────────────────────────────────────┐
//! │ LEB Counter Start&Len  │  LEB Lamport Start&Len              │◁───┐
//! └────────────────────────┴─────────────────────────────────────┘    │
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
//! │               N LEB128 DeltaOfDelta Timestamps               │
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
//! ┌──────────────────────────────────────────────────────────────┐
//! │             (Encoded with Ops By serde_columnar)             │
//! │                       Delete Start IDs                       │
//! │                                                              │
//! └──────────────────────────────────────────────────────────────┘
//! ┌────────────────┬─────────────────────────────────────────────┐
//! │                │                                             │
//! │Value Bytes Size│                Value Bytes                  │
//! │                │                                             │
//! └────────────────┴─────────────────────────────────────────────┘
//! ```

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
use serde_columnar::{
    columnar, AnyRleDecoder, AnyRleEncoder, BoolRleDecoder, BoolRleEncoder, DeltaOfDeltaDecoder,
    DeltaOfDeltaEncoder, DeltaRleDecoder, DeltaRleEncoder, Itertools,
};
use tracing::info;

use super::block_meta_encode::decode_changes_header;
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

#[derive(Debug, Serialize, Deserialize)]
struct EncodedBlock<'a> {
    counter_start: u32,
    counter_len: u32,
    lamport_start: u32,
    lamport_len: u32,
    n_changes: u32,
    #[serde(borrow)]
    header: Cow<'a, [u8]>,
    // timestamp and commit messages
    #[serde(borrow)]
    change_meta: Cow<'a, [u8]>,
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
    delete_start_ids: Cow<'a, [u8]>,
    #[serde(borrow)]
    values: Cow<'a, [u8]>,
}

fn diagnose_block(block: &EncodedBlock) {
    info!("Diagnosing EncodedBlock:");
    info!("  header {} bytes", block.header.len());
    info!("  change_meta {} bytes", block.change_meta.len());
    info!("  cids: {} bytes", block.cids.len());
    info!("  keys: {} bytes", block.keys.len());
    info!("  positions: {} bytes", block.positions.len());
    info!("  ops: {} bytes", block.ops.len());
    info!("  delete_id_starts: {} bytes", block.delete_start_ids.len());
    info!("  values: {} bytes", block.values.len());
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
    let position_bytes = positions.encode_v2();

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

    let ops_bytes = serde_columnar::to_vec(&EncodedOps { ops: encoded_ops }).unwrap();

    let delete_id_starts_bytes = if del_starts.is_empty() {
        Vec::new()
    } else {
        serde_columnar::to_vec(&EncodedDeleteStartIds {
            delete_start_ids: del_starts,
        })
        .unwrap()
    };
    //      ┌────────────────┬─────────────────────────────────────────────┐
    //      │Value Bytes Size│                Value Bytes                  │
    //      └────────────────┴─────────────────────────────────────────────┘

    // PeerIDs
    let mut peer_register = registers.peer_register;
    // .unwrap_vec();
    // let peer_bytes: Vec<u8> = peers.iter().flat_map(|p| p.to_le_bytes()).collect();

    // Change meta
    let (header, change_meta) = encode_changes(block, &mut peer_register);

    let value_bytes = value_writer.finish();
    let out = EncodedBlock {
        counter_start: block[0].id.counter as u32,
        counter_len: (block.last().unwrap().ctr_end() - block[0].id.counter) as u32,
        lamport_start: block[0].lamport(),
        lamport_len: block.last().unwrap().lamport_end() - block[0].lamport(),
        n_changes: block.len() as u32,
        header: header.into(),
        change_meta: change_meta.into(),
        cids: container_arena.encode().into(),
        keys: keys_bytes.into(),
        positions: position_bytes.into(),
        ops: ops_bytes.into(),
        delete_start_ids: delete_id_starts_bytes.into(),
        values: value_bytes.into(),
    };

    diagnose_block(&out);
    let ans = postcard::to_allocvec(&out).unwrap();
    // info!("block size = {}", ans.len());
    println!("BLOCK SIZE = {}", ans.len());
    ans
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
use crate::oplog::change_store::block_meta_encode::encode_changes;
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
        header,
        counter_len,
        counter_start,
        lamport_len,
        lamport_start,
        ..
    } = doc;
    let ans: ChangesBlockHeader = decode_changes_header(
        &header,
        *n_changes as usize,
        *counter_start as Counter,
        *counter_len as Counter,
        *lamport_start,
        *lamport_len,
    );
    Ok(ans)
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
struct EncodedOp {
    #[columnar(strategy = "DeltaRle")]
    container_index: u32,
    #[columnar(strategy = "DeltaRle")]
    prop: i32,
    #[columnar(strategy = "Rle")]
    value_type: u8,
    #[columnar(strategy = "Rle")]
    len: u32,
}

#[columnar(ser, de)]
struct EncodedOps {
    #[columnar(class = "vec", iter = "EncodedOp")]
    ops: Vec<EncodedOp>,
}

#[columnar(ser, de)]
struct EncodedDeleteStartIds {
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
        n_changes,
        counter_start: first_counter,
        change_meta,
        cids,
        keys,
        ops,
        delete_start_ids,
        values,
        positions,
        ..
    } = doc;
    let n_changes = n_changes as usize;
    let mut changes = Vec::with_capacity(n_changes);
    let timestamp_decoder = DeltaOfDeltaDecoder::<i64>::new(&change_meta);
    let (timestamps, bytes) = timestamp_decoder.take_n_finalize(n_changes).unwrap();
    let commit_msg_len_decoder = AnyRleDecoder::<u32>::new(bytes);
    let (commit_msg_lens, commit_msgs) = commit_msg_len_decoder.take_n_finalize(n_changes).unwrap();
    let mut commit_msg_index = 0;
    let keys = header.keys.get_or_init(|| decode_keys(&keys));
    let decode_arena = ValueDecodeArena {
        peers: &header.peers,
        keys,
    };
    let positions = PositionArena::decode_v2(&positions)?;
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
    let encoded_ops_iters = serde_columnar::iter_from_bytes::<EncodedOps>(&ops).unwrap();
    let op_iter = encoded_ops_iters.ops;
    let encoded_delete_id_starts: EncodedDeleteStartIds = if delete_start_ids.is_empty() {
        EncodedDeleteStartIds {
            delete_start_ids: Vec::new(),
        }
    } else {
        serde_columnar::from_bytes(&delete_start_ids).unwrap()
    };
    let mut del_iter = encoded_delete_id_starts
        .delete_start_ids
        .into_iter()
        .map(Ok);
    for i in 0..n_changes {
        let commit_msg: Option<Arc<str>> = {
            let len = commit_msg_lens[i];
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
            timestamp: timestamps[i] as Timestamp,
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

#[cfg(test)]
mod test {
    use crate::{delta::DeltaValue, LoroDoc};

    #[test]
    pub fn encode_single_text_edit() {
        let doc = LoroDoc::new();
        doc.start_auto_commit();
        doc.get_map("map").insert("x", 100).unwrap();
        // doc.get_text("text").insert(0, "Hi").unwrap();
        // let node = doc.get_tree("tree").create(None).unwrap();
        // doc.get_tree("tree").create(node).unwrap();
        diagnose(&doc);
        doc.get_map("map").insert("y", 20).unwrap();
        diagnose(&doc);
        doc.get_map("map").insert("z", 1000).unwrap();
        diagnose(&doc);
        doc.get_text("text").insert(0, "Hello").unwrap();
        diagnose(&doc);
        doc.get_text("text").insert(2, "He").unwrap();
        diagnose(&doc);
        doc.get_text("text").delete(1, 4).unwrap();
        diagnose(&doc);
        doc.get_text("text").delete(0, 2).unwrap();
        diagnose(&doc);
    }

    fn diagnose(doc: &LoroDoc) {
        let bytes = doc.export_from(&Default::default());
        println!("Old Update bytes {:?}", dev_utils::ByteSize(bytes.length()));

        let bytes = doc.export(crate::loro::ExportMode::Updates {
            from: &Default::default(),
        });
        println!("Update bytes {:?}", dev_utils::ByteSize(bytes.length()));
        // assert!(bytes.len() < 30);

        let bytes = doc.export(crate::loro::ExportMode::Snapshot);
        println!("Snapshot bytes {:?}", dev_utils::ByteSize(bytes.length()));
        // assert!(bytes.len() < 30);

        let json = doc.export_json_updates(&Default::default(), &doc.oplog_vv());
        let json_string = serde_json::to_string(&json.changes).unwrap();
        println!(
            "JSON string bytes {:?}",
            dev_utils::ByteSize(json_string.len())
        );
        let bytes = postcard::to_allocvec(&json.changes).unwrap();
        println!("JSON update bytes {:?}", dev_utils::ByteSize(bytes.len()));
        println!("\n\n")
    }
}
