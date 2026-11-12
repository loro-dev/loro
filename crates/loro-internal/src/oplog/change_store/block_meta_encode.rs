use loro_common::{Counter, Lamport, LoroError, LoroResult, PeerID, ID};
use once_cell::sync::OnceCell;
use rle::HasLength;
use serde_columnar::{
    AnyRleDecoder, AnyRleEncoder, BoolRleDecoder, BoolRleEncoder, DeltaOfDeltaEncoder,
};

use crate::{change::Change, encoding::value_register::ValueRegister, version::Frontiers};

use super::block_encode::ChangesBlockHeader;

pub(crate) fn encode_changes(
    block: &[Change],
    peer_register: &mut ValueRegister<PeerID>,
) -> (Vec<u8>, Vec<u8>) {
    let peer = block[0].peer();
    let mut timestamp_encoder = DeltaOfDeltaEncoder::new();
    let mut lamport_encoder = DeltaOfDeltaEncoder::new();
    let mut commit_msg_len_encoder = AnyRleEncoder::<u32>::new();
    let mut commit_msgs = String::new();
    let mut dep_self_encoder = BoolRleEncoder::new();
    let mut dep_len_encoder = AnyRleEncoder::<usize>::new();
    let mut encoded_deps = EncodedDeps {
        peer_idx: AnyRleEncoder::new(),
        counter: DeltaOfDeltaEncoder::new(),
    };
    // First Counter + Change Len
    let mut lengths_bytes = Vec::new();
    let mut counter = vec![];

    for (i, c) in block.iter().enumerate() {
        counter.push(c.id.counter);
        let is_last = i == block.len() - 1;
        if !is_last {
            leb128::write::unsigned(&mut lengths_bytes, c.atom_len() as u64).unwrap();
            lamport_encoder.append(c.lamport() as i64).unwrap();
        }
        timestamp_encoder.append(c.timestamp()).unwrap();
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
                encoded_deps.counter.append(dep.counter as i64).unwrap();
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

    // TODO: capacity
    let mut ans = Vec::with_capacity(block.len() * 15);
    let _ = leb128::write::unsigned(&mut ans, peer_register.vec().len() as u64);
    ans.extend(peer_register.vec().iter().flat_map(|p| p.to_le_bytes()));
    ans.append(&mut lengths_bytes);
    ans.append(&mut dep_self_encoder.finish().unwrap());
    ans.append(&mut dep_len_encoder.finish().unwrap());
    ans.append(&mut encoded_deps.peer_idx.finish().unwrap());
    ans.append(&mut encoded_deps.counter.finish().unwrap());
    ans.append(&mut lamport_encoder.finish().unwrap());

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
) -> LoroResult<ChangesBlockHeader> {
    if n_changes == 0 {
        return Err(LoroError::DecodeError(
            "Decode block error: empty change block".into(),
        ));
    }

    let mut this_counter = first_counter;
    let peer_num =
        usize::try_from(leb128::read::unsigned(&mut bytes).map_err(|e| {
            LoroError::DecodeError(format!("Decode block error {e}").into_boxed_str())
        })?)
        .map_err(|_| LoroError::DecodeDataCorruptionError)?;
    let peers_bytes_len = peer_num
        .checked_mul(8)
        .ok_or_else(|| LoroError::DecodeError("Decode block error: peer length overflow".into()))?;
    if peer_num == 0 || bytes.len() < peers_bytes_len {
        return Err(LoroError::DecodeError(
            "Decode block error: invalid peer table".into(),
        ));
    }
    let mut peers = Vec::with_capacity(peer_num);
    for i in 0..peer_num {
        let start = 8 * i;
        let peer_id = PeerID::from_le_bytes(
            bytes[start..start + 8]
                .try_into()
                .expect("peer byte range should be checked"),
        );
        peers.push(peer_id);
    }
    let mut bytes = &bytes[peers_bytes_len..];

    // ┌───────────────────┬──────────────────────────────────────────┐    │
    // │ LEB First Counter │         N LEB128 Change AtomLen          │◁───┼─────  Important metadata
    // └───────────────────┴──────────────────────────────────────────┘    │

    // Every change except the last has at least one length byte. Check before
    // any count-sized allocation, including the compressed columns below.
    if n_changes - 1 > bytes.len() {
        return Err(LoroError::DecodeDataCorruptionError);
    }
    let mut lengths = Vec::with_capacity(n_changes);
    for _ in 0..n_changes - 1 {
        lengths.push(
            Counter::try_from(leb128::read::unsigned(&mut bytes).map_err(|e| {
                LoroError::DecodeError(format!("Decode block error {e}").into_boxed_str())
            })?)
            .map_err(|_| LoroError::DecodeDataCorruptionError)?,
        );
    }
    let known_len = lengths.iter().try_fold(0i32, |acc, len| {
        acc.checked_add(*len).ok_or_else(|| {
            LoroError::DecodeError("Decode block error: counter length overflow".into())
        })
    })?;
    lengths.push(counter_len.checked_sub(known_len).ok_or_else(|| {
        LoroError::DecodeError("Decode block error: invalid counter length".into())
    })?);

    // ┌───────────────────┬────────────────────────┬─────────────────┐    │
    // │N DepOnSelf BoolRle│ N Delta Rle Deps Lens  │    N Dep IDs    │◁───┘
    // └───────────────────┴────────────────────────┴─────────────────┘

    let dep_self_decoder = BoolRleDecoder::new(bytes);
    let (dep_self, bytes) = dep_self_decoder
        .take_n_finalize(n_changes)
        .map_err(|_| LoroError::DecodeError("Decode block error: invalid deps".into()))?;
    let dep_len_decoder = AnyRleDecoder::<usize>::new(bytes);
    let (deps_len, bytes) = dep_len_decoder
        .take_n_finalize(n_changes)
        .map_err(|_| LoroError::DecodeError("Decode block error: invalid deps".into()))?;
    let other_dep_num = deps_len.iter().try_fold(0usize, |sum, &len| {
        // These are dependencies on OTHER peers; a frontier has at most one
        // dependency per peer, and the block's own peer is at index zero.
        if len >= peer_num {
            return Err(LoroError::DecodeDataCorruptionError);
        }
        sum.checked_add(len)
            .ok_or(LoroError::DecodeDataCorruptionError)
    })?;
    // Each dependency counter after the first needs at least one bit in the
    // following delta-of-delta column. Even RLE peer indices cannot justify
    // more counters than this byte-derived bound. Check before allocating.
    if other_dep_num.saturating_sub(1) > bytes.len().saturating_mul(8) {
        return Err(LoroError::DecodeDataCorruptionError);
    }
    let dep_peer_decoder = AnyRleDecoder::<usize>::new(bytes);
    let (dep_peers, bytes) = dep_peer_decoder
        .take_n_finalize(other_dep_num)
        .map_err(|_| LoroError::DecodeError("Decode block error: invalid deps".into()))?;
    let mut deps_peers_iter = dep_peers.into_iter();
    let (dep_counters, bytes) = decode_delta_of_delta::<u32>(bytes, other_dep_num)?;
    let mut deps_counters_iter = dep_counters.into_iter();
    let mut deps = Vec::with_capacity(n_changes);
    for i in 0..n_changes {
        let mut f = Frontiers::default();
        if dep_self[i] {
            let dep_counter = this_counter.checked_sub(1).ok_or_else(|| {
                LoroError::DecodeError("Decode block error: invalid self dependency".into())
            })?;
            f.push(ID::new(peers[0], dep_counter))
        }

        let len = deps_len[i];
        for _ in 0..len {
            let peer_idx = deps_peers_iter
                .next()
                .ok_or_else(|| LoroError::DecodeError("Decode block error: invalid deps".into()))?;
            let peer = peers.get(peer_idx).copied().ok_or_else(|| {
                LoroError::DecodeError("Decode block error: invalid peer index".into())
            })?;
            let counter = Counter::try_from(deps_counters_iter.next().ok_or_else(|| {
                LoroError::DecodeError("Decode block error: invalid deps".into())
            })?)
            .map_err(|_| LoroError::DecodeDataCorruptionError)?;
            f.push(ID::new(peer, counter));
        }

        deps.push(f);
        this_counter = this_counter
            .checked_add(lengths[i])
            .ok_or_else(|| LoroError::DecodeError("Decode block error: counter overflow".into()))?;
    }
    let mut counters = Vec::with_capacity(n_changes.saturating_add(1));
    let mut last = first_counter;
    for len in lengths.iter() {
        counters.push(last);
        last = last
            .checked_add(*len)
            .ok_or_else(|| LoroError::DecodeError("Decode block error: counter overflow".into()))?;
    }

    let (mut lamports, rest) = decode_delta_of_delta::<Lamport>(bytes, n_changes - 1)?;
    // the last lamport
    let last_len = *lengths.last().unwrap_or(&0) as u32;
    let last_lamport = lamport_start
        .checked_add(lamport_len)
        .and_then(|end| end.checked_sub(last_len))
        .ok_or_else(|| LoroError::DecodeError("Decode block error: invalid lamport".into()))?;
    lamports.push(last_lamport as Lamport);

    // we need counter range, so encode
    counters.push(
        first_counter
            .checked_add(counter_len)
            .ok_or_else(|| LoroError::DecodeError("Decode block error: counter overflow".into()))?,
    );
    if !rest.is_empty() {
        return Err(LoroError::DecodeDataCorruptionError);
    }

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

/// Decode the existing serde_columnar delta-of-delta wire format without its
/// unchecked bit reads/arithmetic or count-sized allocation before validation.
/// Each value after the optional head consumes at least one bit.
pub(super) fn decode_delta_of_delta<T: TryFrom<i64>>(
    bytes: &[u8],
    count: usize,
) -> LoroResult<(Vec<T>, &[u8])> {
    let corrupt = || LoroError::DecodeDataCorruptionError;
    let (head, bytes): (Option<i64>, _) =
        postcard::take_from_bytes(bytes).map_err(|_| corrupt())?;
    let (&last_used_bit, bits) = bytes.split_first().ok_or_else(corrupt)?;
    if last_used_bit > 8
        || head.is_some() != (count != 0)
        || count.saturating_sub(1) > bits.len().saturating_mul(8)
    {
        return Err(corrupt());
    }
    let mut bit_index = 0usize;
    let mut read = |width: usize| -> LoroResult<u64> {
        let mut value = 0u64;
        for _ in 0..width {
            let byte = *bits.get(bit_index / 8).ok_or_else(corrupt)?;
            value = (value << 1) | u64::from((byte >> (7 - bit_index % 8)) & 1);
            bit_index = bit_index.checked_add(1).ok_or_else(corrupt)?;
        }
        Ok(value)
    };
    let mut values = Vec::with_capacity(count.saturating_add(1).min(4096));
    let mut value = head.unwrap_or(0);
    let mut delta = 0i64;
    if count != 0 {
        values.push(T::try_from(value).map_err(|_| corrupt())?);
    }
    for _ in 1..count {
        if read(1)? != 0 {
            let (width, bias) = if read(1)? == 0 {
                (7, 63)
            } else if read(1)? == 0 {
                (9, 255)
            } else if read(1)? == 0 {
                (12, 2047)
            } else if read(1)? == 0 {
                (21, (1 << 20) - 1)
            } else {
                (64, 0)
            };
            let delta_of_delta = (read(width)? as i64)
                .checked_sub(bias)
                .ok_or_else(corrupt)?;
            delta = delta.checked_add(delta_of_delta).ok_or_else(corrupt)?;
        }
        value = value.checked_add(delta).ok_or_else(corrupt)?;
        values.push(T::try_from(value).map_err(|_| corrupt())?);
    }
    // The last-byte bit count belongs to this column, not the following bytes.
    let expected_last = if bit_index == 0 {
        0
    } else {
        (bit_index - 1) % 8 + 1
    };
    if usize::from(last_used_bit) != expected_last {
        return Err(corrupt());
    }
    Ok((values, &bits[bit_index.div_ceil(8)..]))
}

struct EncodedDeps {
    peer_idx: AnyRleEncoder<u32>,
    counter: DeltaOfDeltaEncoder,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_rejects_unbounded_change_and_dependency_counts() {
        let mut bytes = vec![1];
        bytes.extend(1u64.to_le_bytes());
        assert!(decode_changes_header(&bytes, usize::MAX, 0, 1, 0, 1).is_err());
        for len in [u32::MAX as usize, usize::MAX] {
            let mut header = bytes.clone();
            let mut own = BoolRleEncoder::new();
            own.append(false).unwrap();
            header.extend(own.finish().unwrap());
            let mut deps = AnyRleEncoder::<usize>::new();
            deps.append(len).unwrap();
            header.extend(deps.finish().unwrap());
            assert!(decode_changes_header(&header, 1, 0, 1, 0, 1).is_err());
        }
    }

    #[test]
    fn header_rejects_rle_amplified_dependency_total() {
        let mut bytes = Vec::new();
        leb128::write::unsigned(&mut bytes, 257).unwrap();
        for peer in 0u64..257 {
            bytes.extend(peer.to_le_bytes());
        }
        bytes.extend([1; 255]);
        let mut own = BoolRleEncoder::new();
        let mut deps = AnyRleEncoder::<usize>::new();
        for _ in 0..256 {
            own.append(false).unwrap();
            deps.append(256).unwrap();
        }
        bytes.extend(own.finish().unwrap());
        bytes.extend(deps.finish().unwrap());
        // Individually valid lengths must not imply 65,536 entries from
        // a few bytes of compressed peer indices and missing counter bits.
        let mut peers = AnyRleEncoder::<usize>::new();
        for _ in 0..65536 {
            peers.append(1).unwrap();
        }
        bytes.extend(peers.finish().unwrap());
        assert!(decode_changes_header(&bytes, 256, 0, 256, 0, 256).is_err());
    }

    fn encoded(values: &[i64]) -> Vec<u8> {
        let mut encoder = DeltaOfDeltaEncoder::new();
        for &value in values {
            encoder.append(value).unwrap();
        }
        encoder.finish().unwrap()
    }

    #[test]
    fn checked_delta_columns_match_encoder_and_preserve_following_bytes() {
        // Exercise every delta-of-delta prefix, both signs, empty/singleton,
        // multi-byte bit boundaries, and the full-width signed form.
        for values in [
            vec![],
            vec![0],
            vec![i64::MIN],
            vec![i64::MAX],
            vec![0; 18],
            (0..33).collect(),
            vec![
                0,
                64,
                128,
                65,
                321,
                66,
                2114,
                67,
                1_048_643,
                68,
                1 << 40,
                69,
            ],
            vec![0, i64::MIN],
            vec![0, i64::MAX],
        ] {
            let mut bytes = encoded(&values);
            for end in 0..bytes.len() {
                assert!(decode_delta_of_delta::<i64>(&bytes[..end], values.len()).is_err());
            }
            bytes.extend([0xab, 0xcd]);
            let (actual, rest) = decode_delta_of_delta::<i64>(&bytes, values.len()).unwrap();
            assert_eq!(actual, values);
            assert_eq!(rest, &[0xab, 0xcd]);
        }
    }

    #[test]
    fn checked_delta_columns_reject_corruption_and_arithmetic_overflow() {
        let mut bad_last_bit = encoded(&[1, 1]);
        let (_, rest): (Option<i64>, _) = postcard::take_from_bytes(&bad_last_bit).unwrap();
        let offset = bad_last_bit.len() - rest.len();
        bad_last_bit[offset] = 255;
        assert!(decode_delta_of_delta::<i64>(&bad_last_bit, 2).is_err());
        assert!(decode_delta_of_delta::<i64>(&[0, 0], usize::MAX).is_err());
        assert!(decode_delta_of_delta::<u32>(&encoded(&[-1]), 1).is_err());
        // Valid [0,1,2] bits would overflow when its head becomes i64::MAX.
        let normal = encoded(&[0, 1, 2]);
        let (_, tail): (Option<i64>, _) = postcard::take_from_bytes(&normal).unwrap();
        let mut overflow = postcard::to_allocvec(&Some(i64::MAX)).unwrap();
        overflow.extend(tail);
        assert!(decode_delta_of_delta::<i64>(&overflow, 3).is_err());
        // A second delta-of-delta of +1 overflows the previous i64::MAX delta.
        let normal = encoded(&[0, i64::MAX]);
        let mut overflow = normal;
        let (_, tail): (Option<i64>, _) = postcard::take_from_bytes(&overflow).unwrap();
        let offset = overflow.len() - tail.len();
        // First full-width value consumes 69 bits; append prefix 10 + seven
        // bits for +1. The last byte contains 6 used bits (78 in total).
        overflow[offset] = 6;
        *overflow.last_mut().unwrap() |= 0b00000101;
        overflow.push(0);
        assert!(decode_delta_of_delta::<i64>(&overflow, 3).is_err());
    }
}
