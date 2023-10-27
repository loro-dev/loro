use fxhash::FxHashMap;
use loro_common::PeerID;
use rle::RleVec;

use crate::{change::Change, op::RemoteOp};

pub(crate) type ClientChanges = FxHashMap<PeerID, RleVec<[Change; 0]>>;
pub(crate) type RemoteClientChanges<'a> = FxHashMap<PeerID, Vec<Change<RemoteOp<'a>>>>;

mod encode_changes;
mod encode_enhanced;
mod encode_updates;

use rle::HasLength;

use crate::{oplog::OpLog, LoroError, VersionVector};

use self::{
    encode_changes::{decode_oplog_changes, encode_oplog_changes},
    encode_updates::decode_oplog_updates,
};

pub(crate) use encode_enhanced::{decode_oplog_v2, encode_oplog_v2};
pub(crate) use encode_updates::encode_oplog_updates;

pub(crate) const COMPRESS_RLE_THRESHOLD: usize = 20 * 1024;
// TODO: Test this threshold
#[cfg(not(test))]
pub(crate) const UPDATE_ENCODE_THRESHOLD: usize = 512;
#[cfg(test)]
pub(crate) const UPDATE_ENCODE_THRESHOLD: usize = 16;
pub(crate) const MAGIC_BYTES: [u8; 4] = [0x6c, 0x6f, 0x72, 0x6f];
pub(crate) const ENCODE_SCHEMA_VERSION: u8 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EncodeMode {
    // This is a config option, it won't be used in encoding.
    Auto = 255,
    Updates = 0,
    RleUpdates = 1,
    Snapshot = 2,
    CompressedRleUpdates = 3,
    RleUpdatesV2 = 4,
    CompressedRleUpdatesV2 = 5,
}

impl EncodeMode {
    pub fn to_byte(self) -> u8 {
        match self {
            EncodeMode::Auto => 255,
            EncodeMode::Updates => 0,
            EncodeMode::RleUpdates => 1,
            EncodeMode::Snapshot => 2,
            EncodeMode::CompressedRleUpdates => 3,
            EncodeMode::RleUpdatesV2 => 4,
            EncodeMode::CompressedRleUpdatesV2 => 5,
        }
    }
}

impl TryFrom<u8> for EncodeMode {
    type Error = LoroError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(EncodeMode::Updates),
            1 => Ok(EncodeMode::RleUpdates),
            2 => Ok(EncodeMode::Snapshot),
            3 => Ok(EncodeMode::CompressedRleUpdates),
            4 => Ok(EncodeMode::RleUpdatesV2),
            5 => Ok(EncodeMode::CompressedRleUpdatesV2),
            _ => Err(LoroError::DecodeError("Unknown encode mode".into())),
        }
    }
}

pub(crate) fn encode_oplog(oplog: &OpLog, vv: &VersionVector, mode: EncodeMode) -> Vec<u8> {
    let version = ENCODE_SCHEMA_VERSION;
    let mut ans = Vec::from(MAGIC_BYTES);
    // maybe u8 is enough
    ans.push(version);
    let mode = match mode {
        EncodeMode::Auto => {
            let self_vv = oplog.vv();
            let diff = self_vv.diff(vv);
            let update_total_len = diff
                .left
                .values()
                .map(|value| value.atom_len())
                .sum::<usize>();

            // EncodeMode::RleUpdates(vv)
            if update_total_len <= UPDATE_ENCODE_THRESHOLD {
                EncodeMode::Updates
            } else if update_total_len <= COMPRESS_RLE_THRESHOLD {
                EncodeMode::RleUpdatesV2
            } else {
                EncodeMode::CompressedRleUpdatesV2
            }
        }
        mode => mode,
    };

    let encoded = match &mode {
        EncodeMode::Updates => encode_oplog_updates(oplog, vv),
        EncodeMode::RleUpdates => encode_oplog_changes(oplog, vv),
        EncodeMode::CompressedRleUpdates => {
            let bytes = encode_oplog_changes(oplog, vv);
            miniz_oxide::deflate::compress_to_vec(&bytes, 7)
        }
        EncodeMode::RleUpdatesV2 => encode_oplog_v2(oplog, vv),
        EncodeMode::CompressedRleUpdatesV2 => {
            let bytes = encode_oplog_v2(oplog, vv);
            miniz_oxide::deflate::compress_to_vec(&bytes, 7)
        }
        _ => unreachable!(),
    };
    ans.push(mode.to_byte());
    ans.extend(encoded);
    ans
}

pub(crate) fn decode_oplog(oplog: &mut OpLog, input: &[u8]) -> Result<(), LoroError> {
    if input.len() < 6 {
        return Err(LoroError::DecodeError("".into()));
    }

    let (magic_bytes, input) = input.split_at(4);
    let magic_bytes: [u8; 4] = magic_bytes.try_into().unwrap();
    if magic_bytes != MAGIC_BYTES {
        return Err(LoroError::DecodeError("Invalid header bytes".into()));
    }
    let (version, input) = input.split_at(1);
    if version != [ENCODE_SCHEMA_VERSION] {
        return Err(LoroError::DecodeError("Invalid version".into()));
    }

    let mode: EncodeMode = input[0].try_into()?;
    let decoded = &input[1..];
    debug_log::debug_dbg!(&mode);
    match mode {
        EncodeMode::Updates => decode_oplog_updates(oplog, decoded),
        EncodeMode::RleUpdates => decode_oplog_changes(oplog, decoded),
        EncodeMode::CompressedRleUpdates => miniz_oxide::inflate::decompress_to_vec(decoded)
            .map_err(|_| LoroError::DecodeError("Invalid compressed data".into()))
            .and_then(|bytes| decode_oplog_changes(oplog, &bytes)),
        EncodeMode::Snapshot => unimplemented!(),
        EncodeMode::RleUpdatesV2 => decode_oplog_v2(oplog, decoded),
        EncodeMode::CompressedRleUpdatesV2 => miniz_oxide::inflate::decompress_to_vec(decoded)
            .map_err(|_| LoroError::DecodeError("Invalid compressed data".into()))
            .and_then(|bytes| decode_oplog_v2(oplog, &bytes)),
        EncodeMode::Auto => unreachable!(),
    }
}
