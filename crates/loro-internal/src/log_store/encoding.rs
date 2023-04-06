mod encode_changes;
mod encode_snapshot;
mod encode_updates;
mod utils;

use fxhash::FxHashMap;
use rle::HasLength;

use crate::{
    context::Context, dag::Dag, event::EventDiff, hierarchy::Hierarchy, LogStore, LoroError,
    VersionVector,
};

use self::{encode_snapshot::Snapshot, utils::BatchSnapshotSelector};

use super::RemoteClientChanges;

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

pub(crate) trait EncodeBuffer: Sized {
    fn calc_start_vv(&mut self) -> VersionVector;
    fn calc_end_vv(&mut self) -> VersionVector;
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

impl LoroEncoder {
    pub(crate) fn encode_context<C: Context>(ctx: &C, mode: EncodeMode) -> Vec<u8> {
        let store = ctx.log_store();
        let store = store.try_read().unwrap();
        Self::encode(&store, mode)
    }

    pub(crate) fn encode(store: &LogStore, mode: EncodeMode) -> Vec<u8> {
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
        let mut snapshot_events = Vec::new();
        let mut snapshot_selector = BatchSnapshotSelector::new();
        for input in batch {
            let (magic_bytes, input) = input.split_at(4);
            let magic_bytes: [u8; 4] = magic_bytes.try_into().unwrap();
            if magic_bytes != MAGIC_BYTES {
                return Err(LoroError::DecodeError("Invalid header bytes".into()));
            }
            let (_version, input) = input.split_at(1);
            let mode: ConcreteEncodeMode = input[0].into();
            let decoded = &input[1..];
            let decoded_changes = match mode {
                ConcreteEncodeMode::Updates => {
                    encode_updates::decode_updates_to_inner_format(decoded)?
                }
                ConcreteEncodeMode::RleUpdates => {
                    encode_changes::decode_changes_to_inner_format(decoded)?
                }
                ConcreteEncodeMode::Snapshot => {
                    let snapshot = Snapshot::from_bytes(decoded)?;
                    snapshot_selector.add_snapshot(snapshot);
                    continue;
                }
            };

            for (client, mut new_changes) in decoded_changes {
                // FIXME: changes may not be consecutive
                changes.entry(client).or_default().append(&mut new_changes);
            }
        }

        for snapshot in snapshot_selector.select() {
            let (decoded_changes, events) =
                encode_snapshot::decode_snapshot_to_inner_format(store, hierarchy, snapshot)?;
            if let Some(events) = events {
                snapshot_events = events;
            }
            for (client, mut new_changes) in decoded_changes {
                // FIXME: changes may not be consecutive
                changes.entry(client).or_default().append(&mut new_changes);
            }
        }

        Ok(store
            .import(hierarchy, changes)
            .into_iter()
            .chain(snapshot_events)
            .collect())
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
        let changes = encode_changes::decode_changes_to_inner_format(input)?;
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
        let snapshot = Snapshot::from_bytes(input)?;
        let ans = encode_snapshot::decode_snapshot(store, hierarchy, snapshot);
        debug_log::group_end!();
        ans
    }
}

#[cfg(test)]
mod test {
    use crate::LoroCore;

    #[test]
    fn decode_batch() {
        let mut a = LoroCore::default();
        let mut b = LoroCore::default();
        let mut text = a.get_text("text");
        text.insert(&a, 0, "hello").unwrap();
        let snapshot = a.encode_all();
        let v1 = a.vv_cloned();
        text.insert(&a, 5, " world").unwrap();
        let updates2 = a.encode_from(v1);
        let v2 = a.vv_cloned();
        text.insert(&a, 11, "!!!").unwrap();
        let updates3 = a.encode_from(v2);
        b.decode_batch(&[updates3, updates2, snapshot]).unwrap();
        assert_eq!(a.to_json(), b.to_json());
    }
}
