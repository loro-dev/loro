mod encode_enhanced;
mod encode_reordered;
mod encode_snapshot;
mod encode_updates;

use std::f64::consts::E;

use self::encode_updates::decode_oplog_updates;
use crate::encoding::encode_snapshot::encode_app_snapshot;
use crate::LoroDoc;
use crate::{change::Change, op::RemoteOp};
use crate::{oplog::OpLog, LoroError, VersionVector};
use encode_enhanced::{decode_oplog_v2, encode_oplog_v2};
use encode_updates::encode_oplog_updates;
use fxhash::FxHashMap;
use loro_common::{LoroResult, PeerID};
use rle::HasLength;

pub(crate) type RemoteClientChanges<'a> = FxHashMap<PeerID, Vec<Change<RemoteOp<'a>>>>;
pub(crate) use encode_snapshot::decode_app_snapshot;

const COMPRESS_RLE_THRESHOLD: usize = 20 * 1024;
// TODO: Test this threshold
#[cfg(not(test))]
const UPDATE_ENCODE_THRESHOLD: usize = 32;
#[cfg(test)]
const UPDATE_ENCODE_THRESHOLD: usize = 16;
const MAGIC_BYTES: [u8; 4] = [0x6c, 0x6f, 0x72, 0x6f];
const ENCODE_SCHEMA_VERSION: usize = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EncodeMode {
    // This is a config option, it won't be used in encoding.
    Auto = 255,
    Updates = 0,
    Snapshot = 1,
    RleUpdates = 2,
    CompressedRleUpdates = 3,
    ReorderedRle = 4,
}

impl EncodeMode {
    pub fn to_byte(self) -> u8 {
        match self {
            EncodeMode::Auto => 255,
            EncodeMode::Updates => 0,
            EncodeMode::Snapshot => 1,
            EncodeMode::RleUpdates => 2,
            EncodeMode::CompressedRleUpdates => 3,
            EncodeMode::ReorderedRle => 4,
        }
    }
}

impl TryFrom<u8> for EncodeMode {
    type Error = LoroError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(EncodeMode::Updates),
            1 => Ok(EncodeMode::Snapshot),
            2 => Ok(EncodeMode::RleUpdates),
            3 => Ok(EncodeMode::CompressedRleUpdates),
            4 => Ok(EncodeMode::ReorderedRle),
            _ => Err(LoroError::DecodeError("Unknown encode mode".into())),
        }
    }
}

pub(crate) fn encode_oplog(oplog: &OpLog, vv: &VersionVector, mode: EncodeMode) -> Vec<u8> {
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
                EncodeMode::RleUpdates
            } else {
                EncodeMode::CompressedRleUpdates
            }
        }
        mode => mode,
    };

    let body = match &mode {
        EncodeMode::Updates => encode_oplog_updates(oplog, vv),
        EncodeMode::RleUpdates => encode_oplog_v2(oplog, vv),
        EncodeMode::CompressedRleUpdates => {
            let bytes = encode_oplog_v2(oplog, vv);
            miniz_oxide::deflate::compress_to_vec(&bytes, 7)
        }
        EncodeMode::ReorderedRle => encode_reordered::encode(oplog, vv),
        _ => unreachable!(),
    };

    encode_header_and_body(mode, ENCODE_SCHEMA_VERSION, body)
}

pub(crate) fn decode_oplog(
    oplog: &mut OpLog,
    parsed: ParsedHeaderAndBody,
) -> Result<(), LoroError> {
    let ParsedHeaderAndBody { mode, body, .. } = parsed;
    match mode {
        EncodeMode::Updates => decode_oplog_updates(oplog, body),
        EncodeMode::Snapshot => unimplemented!(),
        EncodeMode::RleUpdates => decode_oplog_v2(oplog, body),
        EncodeMode::CompressedRleUpdates => miniz_oxide::inflate::decompress_to_vec(body)
            .map_err(|_| LoroError::DecodeError("Invalid compressed data".into()))
            .and_then(|bytes| decode_oplog_v2(oplog, &bytes)),
        EncodeMode::ReorderedRle => encode_reordered::decode(oplog, body),
        EncodeMode::Auto => unreachable!(),
    }
}

pub(crate) struct ParsedHeaderAndBody<'a> {
    pub checksum: [u8; 16],
    pub checksum_body: &'a [u8],
    pub mode: EncodeMode,
    pub _version: usize,
    pub body: &'a [u8],
}

impl ParsedHeaderAndBody<'_> {
    /// Return if the checksum is correct.
    fn check_checksum(&self) -> LoroResult<()> {
        if md5::compute(self.checksum_body).0 != self.checksum {
            return Err(LoroError::DecodeDataCorruptionError);
        }

        Ok(())
    }
}

const MIN_HEADER_SIZE: usize = 22;
pub(crate) fn parse_header_and_body(bytes: &[u8]) -> Result<ParsedHeaderAndBody, LoroError> {
    let reader = &bytes;
    if bytes.len() < MIN_HEADER_SIZE {
        return Err(LoroError::DecodeError("Invalid import data".into()));
    }

    let (magic_bytes, reader) = reader.split_at(4);
    let magic_bytes: [u8; 4] = magic_bytes.try_into().unwrap();
    if magic_bytes != MAGIC_BYTES {
        return Err(LoroError::DecodeError("Invalid magic bytes".into()));
    }

    let (checksum, reader) = reader.split_at(16);
    let checksum_body = reader;
    let (mode, mut reader) = reader.split_at(1);
    let mode: EncodeMode = mode[0].try_into()?;
    let version = leb128::read::unsigned(&mut reader).unwrap() as usize;
    if version != ENCODE_SCHEMA_VERSION {
        return Err(LoroError::DecodeError(
            format!("Invalid schema version {}", version).into(),
        ));
    }

    let ans = ParsedHeaderAndBody {
        mode,
        _version: version,
        checksum_body,
        checksum: checksum.try_into().unwrap(),
        body: reader,
    };

    ans.check_checksum()?;
    Ok(ans)
}

fn encode_header_and_body(mode: EncodeMode, version: usize, body: Vec<u8>) -> Vec<u8> {
    let mut ans = Vec::new();
    ans.extend(MAGIC_BYTES);
    let checksum = [0; 16];
    ans.extend_from_slice(&checksum);
    ans.push(mode.to_byte());
    leb128::write::unsigned(&mut ans, version as u64).unwrap();
    ans.extend_from_slice(&body);
    let checksum_body = &ans[20..];
    let checksum = md5::compute(checksum_body).0;
    ans[4..20].copy_from_slice(&checksum);
    ans
}

pub(crate) fn export_snapshot(doc: &LoroDoc) -> Vec<u8> {
    let body = encode_app_snapshot(doc);
    encode_header_and_body(EncodeMode::Snapshot, ENCODE_SCHEMA_VERSION, body)
}
