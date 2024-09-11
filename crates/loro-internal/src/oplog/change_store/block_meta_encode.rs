use std::borrow::Cow;

use loro_common::PeerID;
use rle::HasLength;
use serde::{Deserialize, Serialize};
use serde_columnar::{
    columnar, AnyRleDecoder, AnyRleEncoder, BoolRleDecoder, BoolRleEncoder, DeltaOfDeltaEncoder,
    DeltaRleDecoder, DeltaRleEncoder, Itertools,
};

use crate::{change::Change, encoding::value_register::ValueRegister};

use super::delta_rle_encode::UnsignedDeltaEncoder;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct EncodedBlockMeta<'a> {
    n_changes: u32,
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
}

impl<'a> EncodedBlockMeta<'a> {
    pub(crate) fn from_changes(block: &[Change], mut peer_register: ValueRegister<PeerID>) -> Self {
        let peer = block[0].peer();
        let mut timestamp_encoder = DeltaOfDeltaEncoder::new();
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
        // First Counter + Change Len
        let mut lengths_bytes = Vec::new();
        for c in block {
            leb128::write::unsigned(&mut lengths_bytes, c.atom_len() as u64).unwrap();
        }
        Self {
            n_changes: block.len() as u32,
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
        }
    }
}

struct EncodedDeps {
    peer_idx: AnyRleEncoder<u32>,
    counter: AnyRleEncoder<u32>,
}
