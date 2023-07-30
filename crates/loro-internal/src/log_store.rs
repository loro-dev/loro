pub(crate) mod encoding;

pub(crate) use encoding::{decode_oplog, encode_oplog};
pub use encoding::{EncodeMode, LoroEncoder};
use fxhash::FxHashMap;
use loro_common::PeerID;
use rle::RleVec;

use crate::{change::Change, op::RemoteOp};

pub(crate) type ClientChanges = FxHashMap<PeerID, RleVec<[Change; 0]>>;
pub(crate) type RemoteClientChanges<'a> = FxHashMap<PeerID, Vec<Change<RemoteOp<'a>>>>;
