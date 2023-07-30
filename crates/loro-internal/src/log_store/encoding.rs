mod encode_changes;
mod encode_updates;

use rle::HasLength;

use crate::{oplog::OpLog, LoroError, VersionVector};

use self::{
    encode_changes::{decode_oplog_changes, encode_oplog_changes},
    encode_updates::decode_oplog_updates,
};

pub(crate) use encode_updates::encode_oplog_updates;

// TODO: Test this threshold
pub(crate) const UPDATE_ENCODE_THRESHOLD: usize = 512;
pub(crate) const MAGIC_BYTES: [u8; 4] = [0x6c, 0x6f, 0x72, 0x6f];
pub(crate) const ENCODE_SCHEMA_VERSION: u8 = 0;
pub enum EncodeMode {
    Auto(VersionVector),
    Updates(VersionVector),
    RleUpdates(VersionVector),
    Snapshot,
}

impl EncodeMode {
    pub fn to_byte(&self) -> u8 {
        match self {
            EncodeMode::Auto(_) => unreachable!(),
            EncodeMode::Updates(_) => 0,
            EncodeMode::RleUpdates(_) => 1,
            EncodeMode::Snapshot => 2,
        }
    }
}

pub enum ConcreteEncodeMode {
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
                EncodeMode::RleUpdates(vv)
            } else {
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
