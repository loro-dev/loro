mod encode_reordered;

use crate::op::OpWithId;
use crate::LoroDoc;
use crate::{oplog::OpLog, LoroError, VersionVector};
use loro_common::{IdLpSpan, LoroResult, PeerID};
use num_traits::{FromPrimitive, ToPrimitive};
use rle::{HasLength, Sliceable};
use tracing::debug;
const MAGIC_BYTES: [u8; 4] = *b"loro";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EncodeMode {
    // This is a config option, it won't be used in encoding.
    Auto = 255,
    Rle = 1,
    Snapshot = 2,
}

impl num_traits::FromPrimitive for EncodeMode {
    #[allow(trivial_numeric_casts)]
    #[inline]
    fn from_i64(n: i64) -> Option<Self> {
        if n == EncodeMode::Auto as i64 {
            Some(EncodeMode::Auto)
        } else if n == EncodeMode::Rle as i64 {
            Some(EncodeMode::Rle)
        } else if n == EncodeMode::Snapshot as i64 {
            Some(EncodeMode::Snapshot)
        } else {
            None
        }
    }
    #[inline]
    fn from_u64(n: u64) -> Option<Self> {
        Self::from_i64(n as i64)
    }
}

impl num_traits::ToPrimitive for EncodeMode {
    #[inline]
    #[allow(trivial_numeric_casts)]
    fn to_i64(&self) -> Option<i64> {
        Some(match *self {
            EncodeMode::Auto => EncodeMode::Auto as i64,
            EncodeMode::Rle => EncodeMode::Rle as i64,
            EncodeMode::Snapshot => EncodeMode::Snapshot as i64,
        })
    }
    #[inline]
    fn to_u64(&self) -> Option<u64> {
        self.to_i64().map(|x| x as u64)
    }
}

impl EncodeMode {
    pub fn to_bytes(self) -> [u8; 2] {
        let value = self.to_u16().unwrap();
        value.to_be_bytes()
    }

    pub fn is_snapshot(self) -> bool {
        matches!(self, EncodeMode::Snapshot)
    }
}

impl TryFrom<[u8; 2]> for EncodeMode {
    type Error = LoroError;

    fn try_from(value: [u8; 2]) -> Result<Self, Self::Error> {
        let value = u16::from_be_bytes(value);
        Self::from_u16(value).ok_or(LoroError::IncompatibleFutureEncodingError(value as usize))
    }
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
    /// The `check_idspan` function is used to check if the id span is valid.
    /// If the id span is invalid, the function should return an error that
    /// contains the missing id span.
    check_idspan: &'a dyn Fn(IdLpSpan) -> Result<(), IdLpSpan>,
    /// The `encoder_by_op` function is used to encode an operation.
    encoder_by_op: &'a mut dyn FnMut(OpWithId),
    /// The `record_idspan` function is used to record the id span to track the
    /// encoded order.
    record_idspan: &'a mut dyn FnMut(IdLpSpan),
    register_peer: &'a mut dyn FnMut(PeerID) -> usize,
    #[allow(unused)]
    mode: EncodeMode,
}

impl StateSnapshotEncoder<'_> {
    pub fn encode_op(&mut self, id_span: IdLpSpan, get_op: impl FnOnce() -> OpWithId) {
        if let Err(span) = (self.check_idspan)(id_span) {
            let mut op = get_op();
            if span == id_span {
                (self.encoder_by_op)(op);
            } else {
                debug_assert_eq!(span.lamport.start, id_span.lamport.start);
                op.op = op.op.slice(span.atom_len(), op.op.atom_len());
                (self.encoder_by_op)(op);
            }
        }

        (self.record_idspan)(id_span);
    }

    #[allow(unused)]
    pub fn mode(&self) -> EncodeMode {
        self.mode
    }

    pub(crate) fn register_peer(&mut self, peer: PeerID) -> usize {
        (self.register_peer)(peer)
    }
}

pub(crate) struct StateSnapshotDecodeContext<'a> {
    pub oplog: &'a OpLog,
    pub peers: &'a [PeerID],
    pub ops: &'a mut dyn Iterator<Item = OpWithId>,
    #[allow(unused)]
    pub blob: &'a [u8],
    pub mode: EncodeMode,
}

pub(crate) fn encode_oplog(oplog: &OpLog, vv: &VersionVector, mode: EncodeMode) -> Vec<u8> {
    let mode = match mode {
        EncodeMode::Auto => EncodeMode::Rle,
        mode => mode,
    };

    let body = match &mode {
        EncodeMode::Rle => encode_reordered::encode_updates(oplog, vv),
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
        EncodeMode::Rle | EncodeMode::Snapshot => encode_reordered::decode_updates(oplog, body),
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
    debug!("encode_snapshot: header_and_body");
    encode_header_and_body(EncodeMode::Snapshot, body)
}

pub(crate) fn decode_snapshot(
    doc: &LoroDoc,
    mode: EncodeMode,
    body: &[u8],
) -> Result<(), LoroError> {
    match mode {
        EncodeMode::Snapshot => encode_reordered::decode_snapshot(doc, body),
        _ => unreachable!(),
    }
}
