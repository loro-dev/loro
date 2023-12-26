mod encode_enhanced;
mod encode_reordered;
mod encode_snapshot;
mod encode_snapshot_reordered;
mod encode_updates;

use self::encode_updates::decode_oplog_updates;
use crate::encoding::encode_snapshot::encode_app_snapshot;
use crate::op::{Op, OpWithId};
use crate::LoroDoc;
use crate::{change::Change, op::RemoteOp};
use crate::{oplog::OpLog, LoroError, VersionVector};
use encode_enhanced::{decode_oplog_v2, encode_oplog_v2};
use encode_updates::encode_oplog_updates;
use fxhash::FxHashMap;
use loro_common::{HasCounter, IdSpan, LoroResult, PeerID};
use rle::{HasLength, Sliceable};

pub(crate) type RemoteClientChanges<'a> = FxHashMap<PeerID, Vec<Change<RemoteOp<'a>>>>;

#[allow(unused)]
const COMPRESS_RLE_THRESHOLD: usize = 20 * 1024;
// TODO: Test this threshold
#[cfg(not(test))]
const UPDATE_ENCODE_THRESHOLD: usize = 32;
#[cfg(test)]
const UPDATE_ENCODE_THRESHOLD: usize = 16;
const MAGIC_BYTES: [u8; 4] = *b"loro";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EncodeMode {
    // This is a config option, it won't be used in encoding.
    Auto = 255,
    Updates = 0,
    Snapshot = 1,
    RleUpdates = 2,
    CompressedRleUpdates = 3,
    ReorderedRle = 4,
    ReorderedSnapshot = 5,
}

/// The encoder used to encode the container states.
///
/// Each container state can be represented by a sequence of operations.
/// For example, a list state can be represented by a sequence of insert
/// operations that form its current state.
/// We ignore the delete operations.
///
/// We will use a new encoder for each container state.
/// Each container state should call encode_op multiple times until all the
/// operations constituting its current state are encoded.
pub(crate) struct StateSnapshotEncoder<'a> {
    check_idspan: &'a dyn Fn(IdSpan) -> Result<(), IdSpan>,
    encoder_by_op: &'a mut dyn FnMut(OpWithId),
    record_idspan: &'a mut dyn FnMut(IdSpan),
    mode: EncodeMode,
}

impl StateSnapshotEncoder<'_> {
    /// Create a new encoder.
    ///
    /// The `check_idspan` function is used to check if the id span is valid.
    /// If the id span is invalid, the function should return an error that
    /// contains the missing id span.
    ///
    /// The `encoder_by_op` function is used to encode an operation.
    ///
    /// The `record_idspan` function is used to record the id span to track the
    /// encoded order.
    pub fn new<'a>(
        check_idspan: &'a dyn Fn(IdSpan) -> Result<(), IdSpan>,
        encoder_by_op: &'a mut dyn FnMut(OpWithId),
        record_idspan: &'a mut dyn FnMut(IdSpan),
        mode: EncodeMode,
    ) -> StateSnapshotEncoder<'a> {
        StateSnapshotEncoder {
            check_idspan,
            encoder_by_op,
            record_idspan,
            mode,
        }
    }

    pub fn encode_op(&mut self, id_span: IdSpan, get_op: impl FnOnce() -> OpWithId) {
        debug_log::debug_dbg!(id_span);
        if let Err(span) = (self.check_idspan)(id_span) {
            let mut op = get_op();
            if span == id_span {
                (self.encoder_by_op)(op);
            } else {
                debug_assert_eq!(span.ctr_start(), id_span.ctr_start());
                op.op = op.op.slice(span.atom_len(), op.op.atom_len());
                (self.encoder_by_op)(op);
            }
        }

        (self.record_idspan)(id_span);
    }

    pub fn mode(&self) -> EncodeMode {
        self.mode
    }
}

pub(crate) struct StateSnapshotDecodeContext<'a> {
    pub oplog: &'a OpLog,
    pub ops: &'a mut dyn Iterator<Item = OpWithId>,
    pub blob: &'a [u8],
    pub mode: EncodeMode,
}

impl EncodeMode {
    pub fn to_u16(self) -> u16 {
        match self {
            EncodeMode::Auto => 255,
            EncodeMode::Updates => 0,
            EncodeMode::Snapshot => 1,
            EncodeMode::RleUpdates => 2,
            EncodeMode::CompressedRleUpdates => 3,
            EncodeMode::ReorderedRle => 4,
            EncodeMode::ReorderedSnapshot => 5,
        }
    }

    pub fn to_bytes(self) -> [u8; 2] {
        let value = self.to_u16();
        value.to_be_bytes()
    }

    pub fn is_snapshot(self) -> bool {
        matches!(self, EncodeMode::ReorderedSnapshot | EncodeMode::Snapshot)
    }
}

impl TryFrom<[u8; 2]> for EncodeMode {
    type Error = LoroError;

    fn try_from(value: [u8; 2]) -> Result<Self, Self::Error> {
        match value[1] {
            0 => Ok(EncodeMode::Updates),
            1 => Ok(EncodeMode::Snapshot),
            2 => Ok(EncodeMode::RleUpdates),
            3 => Ok(EncodeMode::CompressedRleUpdates),
            4 => Ok(EncodeMode::ReorderedRle),
            5 => Ok(EncodeMode::ReorderedSnapshot),
            _ => Err(LoroError::IncompatibleFutureEncodingError(
                value[0] as usize * 256 + value[1] as usize,
            )),
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
            } else {
                EncodeMode::ReorderedRle
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
        EncodeMode::ReorderedRle => encode_reordered::encode_updates(oplog, vv),
        _ => unreachable!(),
    };

    encode_header_and_body(mode, body)
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
        EncodeMode::ReorderedRle => encode_reordered::decode_updates(oplog, body),
        EncodeMode::ReorderedSnapshot => encode_reordered::decode_updates(oplog, body),
        EncodeMode::Auto => unreachable!(),
    }
}

pub(crate) struct ParsedHeaderAndBody<'a> {
    pub checksum: [u8; 16],
    pub checksum_body: &'a [u8],
    pub mode: EncodeMode,
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
    let (mode_bytes, reader) = reader.split_at(2);
    let mode: EncodeMode = [mode_bytes[0], mode_bytes[1]].try_into()?;

    let ans = ParsedHeaderAndBody {
        mode,
        checksum_body,
        checksum: checksum.try_into().unwrap(),
        body: reader,
    };

    ans.check_checksum()?;
    Ok(ans)
}

fn encode_header_and_body(mode: EncodeMode, body: Vec<u8>) -> Vec<u8> {
    let mut ans = Vec::new();
    ans.extend(MAGIC_BYTES);
    let checksum = [0; 16];
    ans.extend(checksum);
    ans.extend(mode.to_bytes());
    ans.extend(body);
    let checksum_body = &ans[20..];
    let checksum = md5::compute(checksum_body).0;
    ans[4..20].copy_from_slice(&checksum);
    ans
}

pub(crate) fn export_snapshot(doc: &LoroDoc) -> Vec<u8> {
    let body = encode_reordered::encode_snapshot(
        &doc.oplog().try_lock().unwrap(),
        &doc.app_state().try_lock().unwrap(),
        &Default::default(),
    );
    encode_header_and_body(EncodeMode::ReorderedSnapshot, body)
}

pub(crate) fn decode_snapshot(
    doc: &LoroDoc,
    mode: EncodeMode,
    body: &[u8],
    with_state: bool,
) -> Result<(), LoroError> {
    match mode {
        EncodeMode::Snapshot => encode_snapshot::decode_app_snapshot(doc, body, with_state),
        EncodeMode::ReorderedSnapshot => encode_reordered::decode_snapshot(doc, body),
        _ => unreachable!(),
    }
}
