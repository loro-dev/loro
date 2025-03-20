use std::sync::{Arc, Mutex};

use loro_common::PeerID;

use crate::{change::Timestamp, oplog::get_timestamp_now_txn, txn::Transaction, ChangeMeta};

/// The callback of the first commit from a peer.
pub type FirstCommitFromPeerCallback =
    Box<dyn Fn(&FirstCommitFromPeerPayload) -> bool + Send + Sync + 'static>;
pub type PreCommitCallback = Box<dyn Fn(&PreCommitCallbackPayload) -> bool + Send + Sync + 'static>;

/// The payload of the pre commit callback.
#[derive(Debug, Clone)]
pub struct PreCommitCallbackPayload {
    /// The metadata of the change which will be committed.
    pub change_meta: ChangeMeta,
    /// The origin of the commit.
    pub origin: String,
}

/// The payload of the first commit from a peer callback.
#[derive(Debug, Clone)]
pub struct FirstCommitFromPeerPayload {
    /// The peer id of the first commit.
    pub peer: PeerID,
    /// The metadata of the change which will be committed.
    pub change_meta: ChangeMeta,
    /// The modifier of the change. You can modify the change in the callback.
    pub modifier: Arc<Mutex<ChangeModifier>>,
}

#[derive(Debug, Default)]
pub struct ChangeModifier {
    new_msg: Option<Arc<str>>,
    new_timestamp: Option<Timestamp>,
}

impl ChangeModifier {
    pub fn set_msg(&mut self, msg: Arc<str>) {
        self.new_msg = Some(msg);
    }

    pub fn set_timestamp(&mut self, timestamp: Timestamp) {
        self.new_timestamp = Some(timestamp);
    }

    pub fn set_timestamp_now(&mut self) {
        self.new_timestamp = Some(get_timestamp_now_txn());
    }

    pub(crate) fn modify(self, txn: &mut Transaction) {
        if let Some(msg) = self.new_msg {
            txn.set_msg(Some(msg));
        }

        if let Some(timestamp) = self.new_timestamp {
            txn.set_timestamp(timestamp);
        }
    }
}
