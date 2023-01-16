mod encode_changes;
mod encode_snapshot;
mod encode_updates;

use std::io::{Read, Write};

pub use flate2::Compression;
use flate2::{read::DeflateDecoder, write::DeflateEncoder};
use fxhash::FxHashMap;
use num::Zero;
use rle::HasLength;

use crate::{
    dag::Dag, event::RawEvent, hierarchy::Hierarchy, LogStore, LoroCore, LoroError, VersionVector,
};

use super::RemoteClientChanges;

const UPDATE_ENCODE_THRESHOLD: usize = 512;
const MAGIC_BYTES: [u8; 4] = [0x6c, 0x6f, 0x72, 0x6f];
const ENCODE_SCHEMA_VERSION: &str = "1.0";
pub enum EncodeMode {
    Auto(VersionVector),
    Updates(VersionVector),
    RleUpdates(VersionVector),
    Snapshot,
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

pub struct EncodeConfig {
    pub mode: EncodeMode,
    pub compress: Compression,
}

impl EncodeConfig {
    pub fn new(mode: EncodeMode) -> Self {
        Self {
            mode,
            compress: Compression::default(),
        }
    }

    pub fn snapshot() -> Self {
        Self {
            mode: EncodeMode::Snapshot,
            compress: Compression::default(),
        }
    }

    pub fn auto(vv: VersionVector) -> Self {
        Self {
            mode: EncodeMode::Auto(vv),
            compress: Compression::default(),
        }
    }

    pub fn update(vv: VersionVector) -> Self {
        Self {
            mode: EncodeMode::Updates(vv),
            compress: Compression::default(),
        }
    }

    pub fn rle_update(vv: VersionVector) -> Self {
        Self {
            mode: EncodeMode::RleUpdates(vv),
            compress: Compression::default(),
        }
    }

    pub fn from_vv(vv: VersionVector) -> Self {
        let mode = if vv.is_empty() {
            EncodeMode::Snapshot
        } else {
            EncodeMode::Auto(vv)
        };
        Self {
            mode,
            compress: Compression::default(),
        }
    }

    pub fn with_default_compress(self) -> Self {
        self.with_compress(6)
    }

    pub fn with_compress(mut self, level: u32) -> Self {
        self.compress = Compression::new(level);
        self
    }

    pub fn without_compress(mut self) -> Self {
        self.compress = Compression::none();
        self
    }
}

pub struct LoroEncoder;

impl LoroEncoder {
    pub(crate) fn encode(loro: &LoroCore, config: EncodeConfig) -> Vec<u8> {
        let store = loro
            .log_store
            .try_read()
            .map_err(|_| LoroError::LockError)
            .unwrap();
        let version = ENCODE_SCHEMA_VERSION;
        let EncodeConfig { mode, compress } = config;
        let mut ans = Vec::from(MAGIC_BYTES);
        let version_bytes = version.as_bytes();
        // maybe u8 is enough
        ans.push(version_bytes.len() as u8);
        ans.extend_from_slice(version.as_bytes());
        ans.push((compress.level() != 0) as u8);
        let mode = match mode {
            EncodeMode::Auto(vv) => {
                let self_vv = store.vv();
                let diff = self_vv.diff(&vv);
                let update_total_len = diff
                    .left
                    .iter()
                    .map(|(_, value)| value.atom_len())
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
            EncodeMode::Updates(vv) => Self::encode_updates(&store, vv),
            EncodeMode::RleUpdates(vv) => Self::encode_changes(&store, vv),
            EncodeMode::Snapshot => Self::encode_snapshot(&store),
            _ => unreachable!(),
        }
        .unwrap();
        ans.push(mode.to_byte());
        if compress.level() != 0 {
            let mut c = DeflateEncoder::new(&mut ans, compress);
            c.write_all(&encoded).unwrap();
            c.try_finish().unwrap();
        } else {
            ans.extend(encoded);
        };
        ans
    }

    pub fn decode(loro: &mut LoroCore, input: &[u8]) -> Result<Vec<RawEvent>, LoroError> {
        let (magic_bytes, input) = input.split_at(4);
        let magic_bytes: [u8; 4] = magic_bytes.try_into().unwrap();
        if magic_bytes != MAGIC_BYTES {
            return Err(LoroError::DecodeError("Invalid header bytes".into()));
        }
        let (version_len, input) = input.split_at(1);
        // check version
        let (_version, input) = input.split_at(version_len[0] as usize);
        let compress = input[0];
        let mode: ConcreteEncodeMode = input[1].into();
        let mut decoded = Vec::new();
        let decoded = if compress.is_zero() {
            &input[2..]
        } else {
            let mut c = DeflateDecoder::new(&input[2..]);
            c.read_to_end(&mut decoded).unwrap();
            &decoded
        };
        let mut store = loro
            .log_store
            .try_write()
            .map_err(|_| LoroError::LockError)?;
        let mut hierarchy = loro
            .hierarchy
            .try_lock()
            .map_err(|_| LoroError::LockError)?;
        match mode {
            ConcreteEncodeMode::Updates => {
                Self::decode_updates(&mut store, &mut hierarchy, decoded)
            }
            ConcreteEncodeMode::RleUpdates => {
                Self::decode_changes(&mut store, &mut hierarchy, decoded)
            }
            ConcreteEncodeMode::Snapshot => {
                Self::decode_snapshot(&mut store, &mut hierarchy, decoded)
            }
        }
    }

    pub fn decode_batch(
        loro: &mut LoroCore,
        batch: &[Vec<u8>],
    ) -> Result<Vec<RawEvent>, LoroError> {
        let mut store = loro
            .log_store
            .try_write()
            .map_err(|_| LoroError::LockError)?;
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
            let compress = input[0];
            let mode: ConcreteEncodeMode = input[1].into();
            let mut decoded = Vec::new();
            let decoded = if compress.is_zero() {
                &input[2..]
            } else {
                let mut c = DeflateDecoder::new(&input[2..]);
                c.read_to_end(&mut decoded).unwrap();
                &decoded
            };
            let decoded = match mode {
                ConcreteEncodeMode::Updates => {
                    encode_updates::decode_updates_to_inner_format(decoded)?
                }
                ConcreteEncodeMode::RleUpdates => {
                    encode_changes::decode_changes_to_inner_format(decoded)?
                }
                _ => unreachable!("snapshot should not be batched"),
            };

            for (client, mut new_changes) in decoded {
                // FIXME: changes may not be consecutive
                changes.entry(client).or_default().append(&mut new_changes);
            }
        }

        Ok(store.import(&mut loro.hierarchy.lock().unwrap(), changes))
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
    ) -> Result<Vec<RawEvent>, LoroError> {
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
    ) -> Result<Vec<RawEvent>, LoroError> {
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
    ) -> Result<Vec<RawEvent>, LoroError> {
        debug_log::group!("decode snapshot");
        let ans = encode_snapshot::decode_snapshot(store, hierarchy, input);
        debug_log::group_end!();
        ans
    }
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
