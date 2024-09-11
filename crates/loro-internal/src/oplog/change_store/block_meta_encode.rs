use std::borrow::Cow;

use loro_common::{Counter, Lamport, PeerID, ID};
use once_cell::sync::OnceCell;
use rle::HasLength;
use serde::{Deserialize, Serialize};
use serde_columnar::{
    columnar, AnyRleDecoder, AnyRleEncoder, BoolRleDecoder, BoolRleEncoder, DeltaOfDeltaEncoder,
    DeltaRleDecoder, DeltaRleEncoder, Itertools,
};

use crate::{
    change::Change, encoding::value_register::ValueRegister,
    oplog::change_store::delta_rle_encode::UnsignedDeltaDecoder, version::Frontiers,
};

use super::{block_encode::ChangesBlockHeader, delta_rle_encode::UnsignedDeltaEncoder};

pub(crate) fn encode_changes(
    block: &[Change],
    peer_register: &mut ValueRegister<PeerID>,
) -> (Vec<u8>, Vec<u8>) {
    let peer = block[0].peer();
    let mut timestamp_encoder = DeltaOfDeltaEncoder::new();
    let mut lamport_encoder = UnsignedDeltaEncoder::new(block.len() * 2 + 4);
    let mut commit_msg_len_encoder = AnyRleEncoder::<u32>::new();
    let mut commit_msgs = String::new();
    let mut dep_self_encoder = BoolRleEncoder::new();
    let mut dep_len_encoder = AnyRleEncoder::<usize>::new();
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
                c.deps().len() - 1
            } else {
                c.deps().len()
            })
            .unwrap();
    }
    // First Counter + Change Len
    let mut lengths_bytes = Vec::new();
    for c in block {
        leb128::write::unsigned(&mut lengths_bytes, c.atom_len() as u64).unwrap();
    }

    // TODO: capacity
    let mut ans = Vec::with_capacity(block.len() * 15);
    leb128::write::unsigned(&mut ans, peer_register.vec().len() as u64);
    ans.extend(peer_register.vec().iter().flat_map(|p| p.to_le_bytes()));
    ans.append(&mut lengths_bytes);
    ans.append(&mut dep_self_encoder.finish().unwrap());
    ans.append(&mut dep_len_encoder.finish().unwrap());
    ans.append(&mut encoded_deps.peer_idx.finish().unwrap());
    ans.append(&mut encoded_deps.counter.finish().unwrap());
    ans.append(&mut lamport_encoder.finish().0);

    let mut t = timestamp_encoder.finish().unwrap();
    let mut cml = commit_msg_len_encoder.finish().unwrap();
    let mut cms = commit_msgs.into_bytes();
    let mut meta = Vec::with_capacity(t.len() + cml.len() + cms.len());
    meta.append(&mut t);
    meta.append(&mut cml);
    meta.append(&mut cms);

    (ans, meta)
}

pub(crate) fn decode_changes_header(
    mut bytes: &[u8],
    n_changes: usize,
    first_counter: Counter,
    counter_len: Counter,
    lamport_start: Lamport,
    lamport_len: Lamport,
) -> ChangesBlockHeader {
    let mut this_counter = first_counter;
    let peer_num = leb128::read::unsigned(&mut bytes).unwrap() as usize;
    let mut peers = Vec::with_capacity(peer_num);
    for i in 0..peer_num {
        let peer_id = PeerID::from_le_bytes((&bytes[(8 * i)..(8 * (i + 1))]).try_into().unwrap());
        peers.push(peer_id);
    }
    let mut bytes = &bytes[8 * peer_num..];

    // ┌───────────────────┬──────────────────────────────────────────┐    │
    // │ LEB First Counter │         N LEB128 Change AtomLen          │◁───┼─────  Important metadata
    // └───────────────────┴──────────────────────────────────────────┘    │

    let mut lengths = Vec::with_capacity(n_changes);
    for _ in 0..n_changes {
        lengths.push(leb128::read::unsigned(&mut bytes).unwrap() as Counter);
    }

    // ┌───────────────────┬────────────────────────┬─────────────────┐    │
    // │N DepOnSelf BoolRle│ N Delta Rle Deps Lens  │    N Dep IDs    │◁───┘
    // └───────────────────┴────────────────────────┴─────────────────┘

    let dep_self_decoder = BoolRleDecoder::new(bytes);
    let (dep_self, bytes) = dep_self_decoder.take_n_finalize(n_changes).unwrap();
    let dep_len_decoder = AnyRleDecoder::<usize>::new(bytes);
    let (deps_len, bytes) = dep_len_decoder.take_n_finalize(n_changes).unwrap();
    let other_dep_num = deps_len.iter().sum::<usize>();
    let dep_peer_decoder = AnyRleDecoder::<usize>::new(bytes);
    let (dep_peers, bytes) = dep_peer_decoder.take_n_finalize(other_dep_num).unwrap();
    let mut deps_peers_iter = dep_peers.into_iter();
    let dep_counter_decoder = AnyRleDecoder::<u32>::new(bytes);
    let (dep_counters, bytes) = dep_counter_decoder.take_n_finalize(other_dep_num).unwrap();
    let mut deps_counters_iter = dep_counters.into_iter();
    let mut deps = Vec::with_capacity(n_changes);
    for i in 0..n_changes {
        let mut f = Frontiers::default();
        if dep_self[i] {
            f.push(ID::new(peers[0], this_counter - 1))
        }

        let len = deps_len[i] as usize;
        for _ in 0..len {
            let peer_idx = deps_peers_iter.next().unwrap();
            let peer = peers[peer_idx];
            let counter = deps_counters_iter.next().unwrap() as Counter;
            f.push(ID::new(peer, counter));
        }

        deps.push(f);
        this_counter += lengths[i];
    }
    let mut counters = Vec::with_capacity(n_changes + 1);
    let mut last = first_counter;
    for i in 0..n_changes {
        counters.push(last);
        last += lengths[i];
    }
    counters.push(last);
    assert_eq!(last, (first_counter + counter_len) as Counter);

    let mut lamport_decoder = UnsignedDeltaDecoder::new(bytes, n_changes);
    let mut lamports = Vec::with_capacity(n_changes + 1);
    for _ in 0..n_changes {
        lamports.push(lamport_decoder.next().unwrap() as Lamport);
    }
    let last_lamport = *lamports.last().unwrap();
    lamports.push(last_lamport + lengths.last().copied().unwrap() as Lamport);
    assert_eq!(
        *lamports.last().unwrap(),
        (lamport_start + lamport_len) as Lamport
    );
    let rest = lamport_decoder.rest();
    debug_assert!(rest.is_empty());
    ChangesBlockHeader {
        peer: peers[0],
        counter: first_counter,
        n_changes,
        peers,
        counters,
        deps_groups: deps,
        lamports,
        keys: OnceCell::new(),
        cids: OnceCell::new(),
    }
}

struct EncodedDeps {
    peer_idx: AnyRleEncoder<u32>,
    counter: AnyRleEncoder<u32>,
}