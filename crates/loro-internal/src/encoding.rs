pub(crate) mod arena;
pub(crate) mod fast_snapshot;
pub(crate) mod json_schema;
mod outdated_encode_reordered;
mod shallow_snapshot;
pub(crate) mod value;
pub(crate) mod value_register;
pub(crate) use outdated_encode_reordered::{
    decode_op, encode_op, get_op_prop, EncodedDeleteStartId, IterableEncodedDeleteStartId,
};
use outdated_encode_reordered::{import_changes_to_oplog, ImportChangesResult};
pub(crate) use value::OwnedValue;

use crate::op::OpWithId;
use crate::version::{Frontiers, VersionRange};
use crate::LoroDoc;
use crate::{oplog::OpLog, LoroError, VersionVector};
use loro_common::{HasIdSpan, IdLpSpan, IdSpan, LoroEncodeError, LoroResult, PeerID, ID};
use num_traits::{FromPrimitive, ToPrimitive};
use rle::{HasLength, Sliceable};
use std::borrow::Cow;

/// The mode of the export.
///
/// Loro CRDT internally consists of two parts: document history and current document state.
/// The export modes offer various options to meet different requirements.
///
/// - CRDT property: Documents maintain consistent states when they receive the same set of updates.
/// - In real-time collaboration, peers typically only need to synchronize updates
///   (operations/history) to achieve consistency.
///
/// ## Update Export
///
/// - Exports only the history part, containing multiple operations.
/// - Suitable for real-time collaboration scenarios where peers only need to synchronize updates.
///
/// ## Snapshot Export
///
/// ### Default Snapshot
///
/// - Includes complete history and current full state.
///
/// ### Shallow Snapshot
///
/// - Contains the complete current state.
/// - Retains partial history starting from a specified version.
///
/// ### State-only Snapshot
///
/// - Exports the state of the target version.
/// - Includes a minimal set of operation history.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum ExportMode<'a> {
    /// It contains the full history and the current state of the document.
    Snapshot,
    /// It contains the history since the `from` version vector.
    Updates { from: Cow<'a, VersionVector> },
    /// This mode exports the history in the specified range.
    UpdatesInRange { spans: Cow<'a, [IdSpan]> },
    /// The shallow snapshot only contains the history since the target frontiers
    ShallowSnapshot(Cow<'a, Frontiers>),
    /// The state only snapshot exports the state of the target version
    /// with a minimal set of history (a few ops).
    ///
    /// It's a shallow snapshot with depth=1 at the target version.
    /// If the target version is None, it will use the latest version as the target version.
    StateOnly(Option<Cow<'a, Frontiers>>),
    /// The snapshot at the specified frontiers. It contains the full history
    /// till the target frontiers and the state at the target frontiers.
    SnapshotAt { version: Cow<'a, Frontiers> },
}

impl<'a> ExportMode<'a> {
    /// It contains the full history and the current state of the document.
    pub fn snapshot() -> Self {
        ExportMode::Snapshot
    }

    /// It contains the history since the `from` version vector.
    pub fn updates(from: &'a VersionVector) -> Self {
        ExportMode::Updates {
            from: Cow::Borrowed(from),
        }
    }

    /// It contains the history since the `from` version vector.
    pub fn updates_owned(from: VersionVector) -> Self {
        ExportMode::Updates {
            from: Cow::Owned(from),
        }
    }

    /// It contains all the history of the document.
    pub fn all_updates() -> Self {
        ExportMode::Updates {
            from: Cow::Owned(Default::default()),
        }
    }

    /// This mode exports the history in the specified range.
    pub fn updates_in_range(spans: impl Into<Cow<'a, [IdSpan]>>) -> Self {
        ExportMode::UpdatesInRange {
            spans: spans.into(),
        }
    }

    /// The shallow snapshot only contains the history since the target frontiers.
    pub fn shallow_snapshot(frontiers: &'a Frontiers) -> Self {
        ExportMode::ShallowSnapshot(Cow::Borrowed(frontiers))
    }

    /// The shallow snapshot only contains the history since the target frontiers.
    pub fn shallow_snapshot_owned(frontiers: Frontiers) -> Self {
        ExportMode::ShallowSnapshot(Cow::Owned(frontiers))
    }

    /// The shallow snapshot only contains the history since the target frontiers.
    pub fn shallow_snapshot_since(id: ID) -> Self {
        let frontiers = Frontiers::from_id(id);
        ExportMode::ShallowSnapshot(Cow::Owned(frontiers))
    }

    /// The state only snapshot exports the state of the target version
    /// with a minimal set of history (a few ops).
    ///
    /// It's a shallow snapshot with depth=1 at the target version.
    /// If the target version is None, it will use the latest version as the target version.
    pub fn state_only(frontiers: Option<&'a Frontiers>) -> Self {
        ExportMode::StateOnly(frontiers.map(Cow::Borrowed))
    }

    /// The snapshot at the specified frontiers. It contains the full history
    /// till the target frontiers and the state at the target frontiers.
    pub fn snapshot_at(frontiers: &'a Frontiers) -> Self {
        ExportMode::SnapshotAt {
            version: Cow::Borrowed(frontiers),
        }
    }

    /// This mode exports the history within the specified version vector.
    pub fn updates_till(vv: &VersionVector) -> ExportMode<'static> {
        let mut spans = Vec::with_capacity(vv.len());
        for (peer, counter) in vv.iter() {
            if *counter > 0 {
                spans.push(IdSpan::new(*peer, 0, *counter));
            }
        }

        ExportMode::UpdatesInRange {
            spans: Cow::Owned(spans),
        }
    }
}

const MAGIC_BYTES: [u8; 4] = *b"loro";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EncodeMode {
    // This is a config option, it won't be used in encoding.
    Auto = 255,
    OutdatedRle = 1,
    OutdatedSnapshot = 2,
    FastSnapshot = 3,
    FastUpdates = 4,
}

impl num_traits::FromPrimitive for EncodeMode {
    #[allow(trivial_numeric_casts)]
    #[inline]
    fn from_i64(n: i64) -> Option<Self> {
        match n {
            n if n == EncodeMode::Auto as i64 => Some(EncodeMode::Auto),
            n if n == EncodeMode::OutdatedRle as i64 => Some(EncodeMode::OutdatedRle),
            n if n == EncodeMode::OutdatedSnapshot as i64 => Some(EncodeMode::OutdatedSnapshot),
            n if n == EncodeMode::FastSnapshot as i64 => Some(EncodeMode::FastSnapshot),
            n if n == EncodeMode::FastUpdates as i64 => Some(EncodeMode::FastUpdates),
            _ => None,
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
            EncodeMode::OutdatedRle => EncodeMode::OutdatedRle as i64,
            EncodeMode::OutdatedSnapshot => EncodeMode::OutdatedSnapshot as i64,
            EncodeMode::FastSnapshot => EncodeMode::FastSnapshot as i64,
            EncodeMode::FastUpdates => EncodeMode::FastUpdates as i64,
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
        matches!(
            self,
            EncodeMode::OutdatedSnapshot | EncodeMode::FastSnapshot
        )
    }
}

impl TryFrom<[u8; 2]> for EncodeMode {
    type Error = LoroError;

    fn try_from(value: [u8; 2]) -> Result<Self, Self::Error> {
        let value = u16::from_be_bytes(value);
        Self::from_u16(value).ok_or(LoroError::IncompatibleFutureEncodingError(value as usize))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportStatus {
    pub success: VersionRange,
    pub pending: Option<VersionRange>,
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
        EncodeMode::Auto => EncodeMode::OutdatedRle,
        mode => mode,
    };

    let body = match &mode {
        EncodeMode::OutdatedRle => outdated_encode_reordered::encode_updates(oplog, vv),
        _ => unreachable!(),
    };

    encode_header_and_body(mode, body)
}

pub(crate) fn decode_oplog(
    oplog: &mut OpLog,
    parsed: ParsedHeaderAndBody,
) -> Result<ImportStatus, LoroError> {
    let ParsedHeaderAndBody { mode, body, .. } = parsed;
    let changes = match mode {
        EncodeMode::OutdatedRle | EncodeMode::OutdatedSnapshot => {
            outdated_encode_reordered::decode_updates(oplog, body)
        }
        EncodeMode::FastSnapshot => fast_snapshot::decode_oplog(oplog, body),
        EncodeMode::FastUpdates => fast_snapshot::decode_updates(oplog, body.to_vec().into()),
        EncodeMode::Auto => unreachable!(),
    }?;
    let ImportChangesResult {
        mut imported,
        latest_ids,
        pending_changes,
        changes_that_have_deps_before_shallow_root,
    } = import_changes_to_oplog(changes, oplog);

    let mut pending = VersionRange::default();
    pending_changes.iter().for_each(|c| {
        pending.extends_to_include_id_span(c.id_span());
    });
    // TODO: PERF: should we use hashmap to filter latest_ids with the same peer first?
    oplog.try_apply_pending(latest_ids, Some(&mut imported));
    oplog.import_unknown_lamport_pending_changes(pending_changes)?;
    if !changes_that_have_deps_before_shallow_root.is_empty() {
        return Err(LoroError::ImportUpdatesThatDependsOnOutdatedVersion);
    }
    Ok(ImportStatus {
        success: imported,
        pending: (!pending.is_empty()).then_some(pending),
    })
}

pub(crate) struct ParsedHeaderAndBody<'a> {
    pub checksum: [u8; 16],
    pub checksum_body: &'a [u8],
    pub mode: EncodeMode,
    pub body: &'a [u8],
}

const XXH_SEED: u32 = u32::from_le_bytes(*b"LORO");
impl ParsedHeaderAndBody<'_> {
    /// Return if the checksum is correct.
    fn check_checksum(&self) -> LoroResult<()> {
        match self.mode {
            EncodeMode::OutdatedRle | EncodeMode::OutdatedSnapshot => {
                if md5::compute(self.checksum_body).0 != self.checksum {
                    return Err(LoroError::DecodeChecksumMismatchError);
                }
            }
            EncodeMode::FastSnapshot | EncodeMode::FastUpdates => {
                let expected = u32::from_le_bytes(self.checksum[12..16].try_into().unwrap());
                if xxhash_rust::xxh32::xxh32(self.checksum_body, XXH_SEED) != expected {
                    return Err(LoroError::DecodeChecksumMismatchError);
                }
            }
            EncodeMode::Auto => unreachable!(),
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
    let body = outdated_encode_reordered::encode_snapshot(
        &doc.oplog().try_lock().unwrap(),
        &mut doc.app_state().try_lock().unwrap(),
        &Default::default(),
    );

    encode_header_and_body(EncodeMode::OutdatedSnapshot, body)
}

pub(crate) fn export_fast_snapshot(doc: &LoroDoc) -> Vec<u8> {
    encode_with(EncodeMode::FastSnapshot, &mut |ans| {
        fast_snapshot::encode_snapshot(doc, ans);
        Ok(())
    })
    .unwrap()
}

pub(crate) fn export_snapshot_at(
    doc: &LoroDoc,
    frontiers: &Frontiers,
) -> Result<Vec<u8>, LoroEncodeError> {
    check_target_version_reachable(doc, frontiers)?;
    encode_with(EncodeMode::FastSnapshot, &mut |ans| {
        shallow_snapshot::encode_snapshot_at(doc, frontiers, ans)
    })
}

pub(crate) fn export_fast_updates(doc: &LoroDoc, vv: &VersionVector) -> Vec<u8> {
    encode_with(EncodeMode::FastUpdates, &mut |ans| {
        fast_snapshot::encode_updates(doc, vv, ans);
        Ok(())
    })
    .unwrap()
}

pub(crate) fn export_fast_updates_in_range(oplog: &OpLog, spans: &[IdSpan]) -> Vec<u8> {
    encode_with(EncodeMode::FastUpdates, &mut |ans| {
        fast_snapshot::encode_updates_in_range(oplog, spans, ans);
        Ok(())
    })
    .unwrap()
}

pub(crate) fn export_shallow_snapshot(
    doc: &LoroDoc,
    f: &Frontiers,
) -> Result<Vec<u8>, LoroEncodeError> {
    check_target_version_reachable(doc, f)?;
    encode_with(EncodeMode::FastSnapshot, &mut |ans| {
        shallow_snapshot::export_shallow_snapshot(doc, f, ans)?;
        Ok(())
    })
}

fn check_target_version_reachable(doc: &LoroDoc, f: &Frontiers) -> Result<(), LoroEncodeError> {
    let oplog = doc.oplog.try_lock().unwrap();
    if !oplog.dag.can_export_shallow_snapshot_on(f) {
        return Err(LoroEncodeError::FrontiersNotFound(format!("{:?}", f)));
    }

    Ok(())
}

pub(crate) fn export_state_only_snapshot(
    doc: &LoroDoc,
    f: &Frontiers,
) -> Result<Vec<u8>, LoroEncodeError> {
    check_target_version_reachable(doc, f)?;
    encode_with(EncodeMode::FastSnapshot, &mut |ans| {
        shallow_snapshot::export_state_only_snapshot(doc, f, ans)?;
        Ok(())
    })
}

fn encode_with(
    mode: EncodeMode,
    f: &mut dyn FnMut(&mut Vec<u8>) -> Result<(), LoroEncodeError>,
) -> Result<Vec<u8>, LoroEncodeError> {
    // HEADER
    let mut ans = Vec::with_capacity(MIN_HEADER_SIZE);
    ans.extend(MAGIC_BYTES);
    let checksum = [0; 16];
    ans.extend(checksum);
    ans.extend(mode.to_bytes());

    // BODY
    f(&mut ans)?;

    // CHECKSUM in HEADER
    let checksum_body = &ans[20..];
    let checksum = xxhash_rust::xxh32::xxh32(checksum_body, XXH_SEED);
    ans[16..20].copy_from_slice(&checksum.to_le_bytes());
    Ok(ans)
}

pub(crate) fn decode_snapshot(
    doc: &LoroDoc,
    mode: EncodeMode,
    body: &[u8],
) -> Result<ImportStatus, LoroError> {
    match mode {
        EncodeMode::OutdatedSnapshot => outdated_encode_reordered::decode_snapshot(doc, body)?,
        EncodeMode::FastSnapshot => fast_snapshot::decode_snapshot(doc, body.to_vec().into())?,
        _ => unreachable!(),
    };
    Ok(ImportStatus {
        success: VersionRange::from_vv(&doc.oplog_vv()),
        pending: None,
    })
}

#[derive(Debug, Clone)]
pub struct ImportBlobMetadata {
    /// The partial start version vector.
    ///
    /// Import blob includes all the ops from `partial_start_vv` to `partial_end_vv`.
    /// However, it does not constitute a complete version vector, as it only contains counters
    /// from peers included within the import blob.
    pub partial_start_vv: VersionVector,
    /// The partial end version vector.
    ///
    /// Import blob includes all the ops from `partial_start_vv` to `partial_end_vv`.
    /// However, it does not constitute a complete version vector, as it only contains counters
    /// from peers included within the import blob.
    pub partial_end_vv: VersionVector,
    pub start_timestamp: i64,
    pub start_frontiers: Frontiers,
    pub end_timestamp: i64,
    pub change_num: u32,
    pub is_snapshot: bool,
}

impl LoroDoc {
    /// Decodes the metadata for an imported blob from the provided bytes.
    pub fn decode_import_blob_meta(blob: &[u8]) -> LoroResult<ImportBlobMetadata> {
        outdated_encode_reordered::decode_import_blob_meta(blob)
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use loro_common::{loro_value, ContainerID, ContainerType, LoroValue, ID};

    #[test]
    fn test_value_encode_size() {
        fn assert_size(value: LoroValue, max_size: usize) {
            let size = postcard::to_allocvec(&value).unwrap().len();
            assert!(
                size <= max_size,
                "value: {:?}, size: {}, max_size: {}",
                value,
                size,
                max_size
            );
        }

        assert_size(LoroValue::Null, 1);
        assert_size(LoroValue::I64(1), 2);
        assert_size(LoroValue::Double(1.), 9);
        assert_size(LoroValue::Bool(true), 2);
        assert_size(LoroValue::String("123".to_string().into()), 5);
        assert_size(LoroValue::Binary(vec![1, 2, 3].into()), 5);
        assert_size(
            loro_value!({
                "a": 1,
                "b": 2,
            }),
            10,
        );
        assert_size(loro_value!([1, 2, 3]), 8);
        assert_size(
            LoroValue::Container(ContainerID::new_normal(ID::new(1, 1), ContainerType::Map)),
            5,
        );
        assert_size(
            LoroValue::Container(ContainerID::new_root("a", ContainerType::Map)),
            5,
        );
    }
}
