use std::sync::{Arc, Mutex};

use crate::{
    change::{Change, Timestamp},
    oplog::get_timestamp_now_txn,
    ChangeMeta,
};
use loro_common::PeerID;

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
    /// The modifier of the change. You can modify the change in the callback.
    pub modifier: ChangeModifier,
}

/// The payload of the first commit from a peer callback.
#[derive(Debug, Clone)]
pub struct FirstCommitFromPeerPayload {
    /// The peer id of the first commit.
    pub peer: PeerID,
}

#[derive(Debug, Clone, Default)]
pub struct ChangeModifier(Arc<Mutex<ChangeModifierInner>>);

#[derive(Debug, Default)]
struct ChangeModifierInner {
    new_msg: Option<Arc<str>>,
    new_timestamp: Option<Timestamp>,
}

impl ChangeModifier {
    pub fn set_message(&self, msg: &str) -> &Self {
        self.0.lock().unwrap().new_msg = Some(Arc::from(msg));
        self
    }

    pub fn set_timestamp(&self, timestamp: Timestamp) -> &Self {
        self.0.lock().unwrap().new_timestamp = Some(timestamp);
        self
    }

    pub fn set_timestamp_now(&self) -> &Self {
        self.0.lock().unwrap().new_timestamp = Some(get_timestamp_now_txn());
        self
    }

    pub(crate) fn modify_change(&self, change: &mut Change) {
        let m = self.0.lock().unwrap();
        if let Some(msg) = &m.new_msg {
            change.commit_msg = Some(msg.clone());
        }

        if let Some(timestamp) = m.new_timestamp {
            change.timestamp = timestamp;
        }
    }
}
