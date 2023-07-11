mod encode_changes;
mod encode_snapshot;
mod encode_updates;

use fxhash::FxHashMap;
use rle::HasLength;

use crate::{
    context::Context, dag::Dag, event::EventDiff, hierarchy::Hierarchy, refactor::oplog::OpLog,
    LogStore, LoroError, VersionVector,
};

use self::{
    encode_changes::encode_oplog_changes,
    encode_updates::{decode_oplog_changes, decode_oplog_updates},
};

use super::RemoteClientChanges;
pub(crate) use encode_updates::encode_oplog_updates;

// TODO: Test this threshold
const UPDATE_ENCODE_THRESHOLD: usize = 512;
const MAGIC_BYTES: [u8; 4] = [0x6c, 0x6f, 0x72, 0x6f];
const ENCODE_SCHEMA_VERSION: u8 = 0;
pub enum EncodeMode {
    Auto(VersionVector),
    Updates(VersionVector),
    RleUpdates(VersionVector),
    Snapshot,
}

impl EncodeMode {
    fn to_byte(&self) -> u8 {
        match self {
            EncodeMode::Auto(_) => unreachable!(),
            EncodeMode::Updates(_) => 0,
            EncodeMode::RleUpdates(_) => 1,
            EncodeMode::Snapshot => 2,
        }
    }
}

enum ConcreteEncodeMode {
    Updates = 0,
    RleUpdates = 1,
    Snapshot = 2,
}

impl From<u8> for ConcreteEncodeMode {
    fn from(value: u8) -> Self {
        match value {
            0 => ConcreteEncodeMode::Updates,
            1 => ConcreteEncodeMode::RleUpdates,
            2 => ConcreteEncodeMode::Snapshot,
            _ => unreachable!(),
        }
    }
}

pub struct LoroEncoder;

pub(crate) fn encode_oplog(oplog: &OpLog, mode: EncodeMode) -> Vec<u8> {
    let version = ENCODE_SCHEMA_VERSION;
    let mut ans = Vec::from(MAGIC_BYTES);
    // maybe u8 is enough
    ans.push(version);
    let mode = match mode {
        EncodeMode::Auto(vv) => {
            let self_vv = oplog.vv();
            let diff = self_vv.diff(&vv);
            let update_total_len = diff
                .left
                .values()
                .map(|value| value.atom_len())
                .sum::<usize>();
            if update_total_len > UPDATE_ENCODE_THRESHOLD {
                debug_log::debug_log!("Encode RleUpdates");
                EncodeMode::RleUpdates(vv)
            } else {
                debug_log::debug_log!("Encode Updates");
                EncodeMode::Updates(vv)
            }
        }
        mode => mode,
    };
    let encoded = match &mode {
        EncodeMode::Updates(vv) => encode_oplog_updates(oplog, vv),
        EncodeMode::RleUpdates(vv) => encode_oplog_changes(oplog, vv),
        EncodeMode::Snapshot => unimplemented!(),
        _ => unreachable!(),
    };
    ans.push(mode.to_byte());
    ans.extend(encoded);
    ans
}

pub(crate) fn decode_oplog(oplog: &mut OpLog, input: &[u8]) -> Result<(), LoroError> {
    let (magic_bytes, input) = input.split_at(4);
    let magic_bytes: [u8; 4] = magic_bytes.try_into().unwrap();
    if magic_bytes != MAGIC_BYTES {
        return Err(LoroError::DecodeError("Invalid header bytes".into()));
    }
    let (version, input) = input.split_at(1);
    if version != [ENCODE_SCHEMA_VERSION] {
        return Err(LoroError::DecodeError("Invalid version".into()));
    }

    let mode: ConcreteEncodeMode = input[0].into();
    let decoded = &input[1..];
    match mode {
        ConcreteEncodeMode::Updates => decode_oplog_updates(oplog, decoded),
        ConcreteEncodeMode::RleUpdates => decode_oplog_changes(oplog, decoded),
        ConcreteEncodeMode::Snapshot => unimplemented!(),
    }
}

impl LoroEncoder {
    pub(crate) fn encode_context<C: Context>(ctx: &C, mode: EncodeMode) -> Vec<u8> {
        let store = ctx.log_store();
        let store = store.try_read().unwrap();
        Self::encode(&store, mode)
    }

    pub(crate) fn encode(store: &LogStore, mode: EncodeMode) -> Vec<u8> {
        store.expose_local_change();
        let version = ENCODE_SCHEMA_VERSION;
        let mut ans = Vec::from(MAGIC_BYTES);
        // maybe u8 is enough
        ans.push(version);
        let mode = match mode {
            EncodeMode::Auto(vv) => {
                let self_vv = store.vv();
                let diff = self_vv.diff(&vv);
                let update_total_len = diff
                    .left
                    .values()
                    .map(|value| value.atom_len())
                    .sum::<usize>();
                if update_total_len > UPDATE_ENCODE_THRESHOLD {
                    debug_log::debug_log!("Encode RleUpdates");
                    EncodeMode::RleUpdates(vv)
                } else {
                    debug_log::debug_log!("Encode Updates");
                    EncodeMode::Updates(vv)
                }
            }
            mode => mode,
        };
        let encoded = match &mode {
            EncodeMode::Updates(vv) => Self::encode_updates(store, vv),
            EncodeMode::RleUpdates(vv) => Self::encode_changes(store, vv),
            EncodeMode::Snapshot => Self::encode_snapshot(store),
            _ => unreachable!(),
        }
        .unwrap();
        ans.push(mode.to_byte());
        ans.extend(encoded);
        ans
    }

    pub(crate) fn decode(
        store: &mut LogStore,
        hierarchy: &mut Hierarchy,
        input: &[u8],
    ) -> Result<Vec<EventDiff>, LoroError> {
        let (magic_bytes, input) = input.split_at(4);
        let magic_bytes: [u8; 4] = magic_bytes.try_into().unwrap();
        if magic_bytes != MAGIC_BYTES {
            return Err(LoroError::DecodeError("Invalid header bytes".into()));
        }
        let (_version, input) = input.split_at(1);
        // check version
        let mode: ConcreteEncodeMode = input[0].into();
        let decoded = &input[1..];
        match mode {
            ConcreteEncodeMode::Updates => Self::decode_updates(store, hierarchy, decoded),
            ConcreteEncodeMode::RleUpdates => Self::decode_changes(store, hierarchy, decoded),
            ConcreteEncodeMode::Snapshot => Self::decode_snapshot(store, hierarchy, decoded),
        }
    }

    pub(crate) fn decode_batch(
        store: &mut LogStore,
        hierarchy: &mut Hierarchy,
        batch: &[Vec<u8>],
    ) -> Result<Vec<EventDiff>, LoroError> {
        let mut changes: RemoteClientChanges = FxHashMap::default();
        for input in batch {
            let (magic_bytes, input) = input.split_at(4);
            let magic_bytes: [u8; 4] = magic_bytes.try_into().unwrap();
            if magic_bytes != MAGIC_BYTES {
                return Err(LoroError::DecodeError("Invalid header bytes".into()));
            }
            let (version_len, input) = input.split_at(1);
            // check version
            let (_version, input) = input.split_at(version_len[0] as usize);
            let mode: ConcreteEncodeMode = input[0].into();
            let decoded = &input[1..];
            let decoded2 = match mode {
                ConcreteEncodeMode::Updates => {
                    encode_updates::decode_updates_to_inner_format(decoded)?
                }
                ConcreteEncodeMode::RleUpdates => {
                    encode_changes::decode_changes_to_inner_format(decoded, store)?
                }
                _ => unreachable!("snapshot should not be batched"),
            };

            for (client, mut new_changes) in decoded2 {
                // FIXME: changes may not be consecutive
                changes.entry(client).or_default().append(&mut new_changes);
            }
        }

        Ok(store.import(hierarchy, changes))
    }
}

impl LoroEncoder {
    #[inline]
    fn encode_updates(store: &LogStore, vv: &VersionVector) -> Result<Vec<u8>, LoroError> {
        encode_updates::encode_updates(store, vv)
    }

    #[inline]
    fn decode_updates(
        store: &mut LogStore,
        hierarchy: &mut Hierarchy,
        input: &[u8],
    ) -> Result<Vec<EventDiff>, LoroError> {
        let changes = encode_updates::decode_updates(input)?;
        Ok(store.import(hierarchy, changes))
    }

    #[inline]
    fn encode_changes(store: &LogStore, vv: &VersionVector) -> Result<Vec<u8>, LoroError> {
        encode_changes::encode_changes(store, vv)
    }

    #[inline]
    fn decode_changes(
        store: &mut LogStore,
        hierarchy: &mut Hierarchy,
        input: &[u8],
    ) -> Result<Vec<EventDiff>, LoroError> {
        let changes = encode_changes::decode_changes_to_inner_format(input, store)?;
        Ok(store.import(hierarchy, changes))
    }

    #[inline]
    fn encode_snapshot(store: &LogStore) -> Result<Vec<u8>, LoroError> {
        encode_snapshot::encode_snapshot(store, store.cfg.gc.gc)
    }

    #[inline]
    fn decode_snapshot(
        store: &mut LogStore,
        hierarchy: &mut Hierarchy,
        input: &[u8],
    ) -> Result<Vec<EventDiff>, LoroError> {
        debug_log::group!("decode snapshot");
        let ans = encode_snapshot::decode_snapshot(store, hierarchy, input);
        debug_log::group_end!();
        ans
    }
}
