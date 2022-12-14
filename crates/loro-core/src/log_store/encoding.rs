mod encode_changes;
mod encode_snapshot;
mod encode_updates;

use std::io::{Read, Write};

use flate2::{read::DeflateDecoder, write::DeflateEncoder, Compression};
use num::Zero;

use crate::{dag::Dag, event::RawEvent, LogStore, LoroCore, LoroError, VersionVector};

const UPDATE_ENCODE_THRESHOLD: usize = 5;
const MAGIC_BYTES: [u8; 4] = [0x6c, 0x6f, 0x72, 0x6f];
pub enum EncodeMode {
    Auto(Option<VersionVector>),
    Updates(VersionVector),
    Changes(VersionVector),
    Snapshot,
}

pub struct EncodeConfig {
    mode: EncodeMode,
    compress: Option<u32>,
}

impl EncodeConfig {
    pub fn new(mode: EncodeMode, compress: Option<u32>) -> Self {
        Self { mode, compress }
    }

    pub fn from_vv(vv: Option<VersionVector>) -> Self {
        let mode = if vv.is_none() {
            EncodeMode::Snapshot
        } else {
            EncodeMode::Auto(vv)
        };
        Self {
            mode,
            compress: None,
        }
    }

    pub fn with_default_compress(self) -> Self {
        self.with_compress(6)
    }

    pub fn with_compress(mut self, level: u32) -> Self {
        self.compress = Some(level);
        self
    }
}

pub struct LoroEncoder;

impl LoroEncoder {
    pub(crate) fn encode(loro: &LoroCore, config: EncodeConfig) -> Result<Vec<u8>, LoroError> {
        let version = env!("CARGO_PKG_VERSION");
        let store = loro
            .log_store
            .try_read()
            .map_err(|_| LoroError::LockError)?;
        let EncodeConfig { mode, compress } = config;
        let mut ans = Vec::from(MAGIC_BYTES);
        let version_bytes = version.as_bytes();
        // maybe u8 is enough
        ans.push(version_bytes.len() as u8);
        ans.extend_from_slice(version.as_bytes());
        ans.push(compress.is_some() as u8);
        let mode = match mode {
            EncodeMode::Auto(option_vv) => {
                if let Some(vv) = option_vv {
                    let self_vv = store.vv();
                    let diff = self_vv.diff(&vv);
                    if diff.left.len() > UPDATE_ENCODE_THRESHOLD {
                        EncodeMode::Changes(vv)
                    } else {
                        EncodeMode::Updates(vv)
                    }
                } else {
                    EncodeMode::Snapshot
                }
            }
            mode => mode,
        };
        let encoded = match &mode {
            EncodeMode::Updates(vv) => Self::encode_updates(&store, vv),
            EncodeMode::Changes(vv) => Self::encode_changes(&store, vv),
            EncodeMode::Snapshot => Self::encode_snapshot(&store),
            _ => unreachable!(),
        }?;
        ans.push(mode.to_byte());
        if let Some(level) = compress {
            let mut c = DeflateEncoder::new(&mut ans, Compression::new(level));
            c.write_all(&encoded)
                .map_err(|e| LoroError::DecodeError(e.to_string().into()))?;
            c.try_finish()
                .map_err(|e| LoroError::DecodeError(e.to_string().into()))?;
        } else {
            ans.extend(encoded);
        };
        Ok(ans)
    }

    pub fn decode(loro: &mut LoroCore, input: &[u8]) -> Result<Vec<RawEvent>, LoroError> {
        let mut store = loro
            .log_store
            .try_write()
            .map_err(|_| LoroError::LockError)?;
        let (magic_bytes, input) = input.split_at(4);
        let magic_bytes: [u8; 4] = magic_bytes.try_into().unwrap();
        if magic_bytes != MAGIC_BYTES {
            return Err(LoroError::DecodeError("Invalid header bytes".into()));
        }
        let (version_len, input) = input.split_at(1);
        // check version
        let (_version, input) = input.split_at(version_len[0] as usize);
        let compress = input[0];
        let mode = input[1];
        let mut decoded = Vec::new();
        let decoded = if compress.is_zero() {
            &input[2..]
        } else {
            let mut c = DeflateDecoder::new(&input[2..]);
            c.read_to_end(&mut decoded).unwrap();
            &decoded
        };
        match mode {
            0 => Self::decode_updates(&mut store, decoded),
            1 => Self::decode_changes(&mut store, decoded),
            2 => Self::decode_snapshot(&mut store, decoded),
            _ => unreachable!(),
        }
    }
}

impl LoroEncoder {
    #[inline]
    fn encode_updates(store: &LogStore, vv: &VersionVector) -> Result<Vec<u8>, LoroError> {
        encode_updates::encode_updates(store, vv)
    }

    #[inline]
    fn decode_updates(store: &mut LogStore, input: &[u8]) -> Result<Vec<RawEvent>, LoroError> {
        encode_updates::decode_updates(store, input)
    }

    #[inline]
    fn encode_changes(store: &LogStore, vv: &VersionVector) -> Result<Vec<u8>, LoroError> {
        encode_changes::encode_changes(store, vv)
    }

    #[inline]
    fn decode_changes(store: &mut LogStore, input: &[u8]) -> Result<Vec<RawEvent>, LoroError> {
        encode_changes::decode_changes(store, input)
    }

    #[inline]
    fn encode_snapshot(store: &LogStore) -> Result<Vec<u8>, LoroError> {
        encode_snapshot::encode_snapshot(store, store.cfg.gc.gc)
    }

    #[inline]
    fn decode_snapshot(store: &mut LogStore, input: &[u8]) -> Result<Vec<RawEvent>, LoroError> {
        encode_snapshot::decode_snapshot(store, input)
    }
}

impl EncodeMode {
    fn to_byte(&self) -> u8 {
        match self {
            EncodeMode::Auto(_) => unreachable!(),
            EncodeMode::Updates(_) => 0,
            EncodeMode::Changes(_) => 1,
            EncodeMode::Snapshot => 2,
        }
    }
}
