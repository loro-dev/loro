use std::sync::{Arc, Mutex};

use loro_common::PeerID;

use crate::{
    change::{Change, Timestamp},
    oplog::get_timestamp_now_txn,
    txn::Transaction,
    ChangeMeta,
};

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
    /// The modifier of the change. You can modify the change in the callback.
    pub modifier: ChangeModifier,
}

#[derive(Debug, Clone, Default)]
pub struct ChangeModifier(Arc<Mutex<ChangeModifierInner>>);

impl ChangeModifier {
    pub fn set_msg(&self, msg: String) {
        self.0.lock().unwrap().set_msg(Arc::from(msg));
    }

    pub fn set_timestamp(&self, timestamp: Timestamp) {
        self.0.lock().unwrap().set_timestamp(timestamp);
    }

    pub fn set_timestamp_now(&self) {
        self.0.lock().unwrap().set_timestamp_now();
    }

    pub(crate) fn modify_change(&self, change: &mut Change) {
        self.0.lock().unwrap().modify_change(change);
    }

    pub(crate) fn modify(&self, txn: &mut Transaction) {
        self.0.lock().unwrap().modify(txn);
    }
}

#[derive(Debug, Default)]
struct ChangeModifierInner {
    new_msg: Option<Arc<str>>,
    new_timestamp: Option<Timestamp>,
}

impl ChangeModifierInner {
    fn set_msg(&mut self, msg: Arc<str>) {
        self.new_msg = Some(msg);
    }

    fn set_timestamp(&mut self, timestamp: Timestamp) {
        self.new_timestamp = Some(timestamp);
    }

    fn set_timestamp_now(&mut self) {
        self.new_timestamp = Some(get_timestamp_now_txn());
    }

    fn modify_change(&self, change: &mut Change) {
        if let Some(msg) = &self.new_msg {
            change.commit_msg = Some(msg.clone());
        }

        if let Some(timestamp) = self.new_timestamp {
            change.timestamp = timestamp;
        }
    }

    fn modify(&self, txn: &mut Transaction) {
        if let Some(msg) = &self.new_msg {
            txn.set_msg(Some(msg.clone()));
        }

        if let Some(timestamp) = self.new_timestamp {
            txn.set_timestamp(timestamp);
        }
    }
}
