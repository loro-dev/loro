#![allow(deprecated)]
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use loro::PeerID;

use crate::{LoroValue, LoroValueLike};
pub struct Awareness(Mutex<loro::awareness::Awareness>);

impl Awareness {
    pub fn new(peer: PeerID, timeout: i64) -> Self {
        Self(Mutex::new(loro::awareness::Awareness::new(peer, timeout)))
    }

    pub fn encode(&self, peers: &[PeerID]) -> Vec<u8> {
        self.0.lock().unwrap().encode(peers)
    }

    pub fn encode_all(&self) -> Vec<u8> {
        self.0.lock().unwrap().encode_all()
    }

    pub fn apply(&self, encoded_peers_info: &[u8]) -> AwarenessPeerUpdate {
        let (updated, added) = self.0.lock().unwrap().apply(encoded_peers_info);
        AwarenessPeerUpdate { updated, added }
    }

    pub fn set_local_state(&self, value: Arc<dyn LoroValueLike>) {
        self.0
            .lock()
            .unwrap()
            .set_local_state(value.as_loro_value());
    }

    pub fn get_local_state(&self) -> Option<LoroValue> {
        self.0.lock().unwrap().get_local_state().map(|x| x.into())
    }

    pub fn remove_outdated(&self) -> Vec<PeerID> {
        self.0.lock().unwrap().remove_outdated()
    }

    pub fn get_all_states(&self) -> HashMap<PeerID, PeerInfo> {
        self.0
            .lock()
            .unwrap()
            .get_all_states()
            .iter()
            .map(|(p, i)| (*p, i.into()))
            .collect()
    }

    pub fn peer(&self) -> PeerID {
        self.0.lock().unwrap().peer()
    }
}

pub struct AwarenessPeerUpdate {
    pub updated: Vec<PeerID>,
    pub added: Vec<PeerID>,
}

pub struct PeerInfo {
    pub state: LoroValue,
    pub counter: i32,
    // This field is generated locally
    pub timestamp: i64,
}

impl From<&loro::awareness::PeerInfo> for PeerInfo {
    fn from(value: &loro::awareness::PeerInfo) -> Self {
        Self {
            state: value.state.clone().into(),
            counter: value.counter,
            timestamp: value.timestamp,
        }
    }
}
