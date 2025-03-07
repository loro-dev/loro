use fxhash::FxHashMap;
use loro_common::{LoroValue, PeerID};
use serde::{Deserialize, Serialize};

use crate::change::{get_sys_timestamp, Timestamp};
use crate::{SubscriberSetWithQueue, Subscription};

/// `Awareness` is a structure that tracks the ephemeral state of peers.
///
/// It can be used to synchronize cursor positions, selections, and the names of the peers.
///
/// The state of a specific peer is expected to be removed after a specified timeout. Use
/// `remove_outdated` to eliminate outdated states.
#[derive(Debug, Clone)]
#[deprecated(since = "1.4.6", note = "Use `EphemeralStore` instead.")]
pub struct Awareness {
    peer: PeerID,
    peers: FxHashMap<PeerID, PeerInfo>,
    timeout: i64,
}

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub state: LoroValue,
    pub counter: i32,
    // This field is generated locally
    pub timestamp: i64,
}

#[derive(Serialize, Deserialize)]
struct EncodedPeerInfo {
    peer: PeerID,
    counter: i32,
    record: LoroValue,
}

#[allow(deprecated)]
impl Awareness {
    pub fn new(peer: PeerID, timeout: i64) -> Awareness {
        Awareness {
            peer,
            timeout,
            peers: FxHashMap::default(),
        }
    }

    pub fn encode(&self, peers: &[PeerID]) -> Vec<u8> {
        let mut peers_info = Vec::new();
        let now = get_sys_timestamp() as Timestamp;
        for peer in peers {
            if let Some(peer_info) = self.peers.get(peer) {
                if now - peer_info.timestamp > self.timeout {
                    continue;
                }

                let encoded_peer_info = EncodedPeerInfo {
                    peer: *peer,
                    record: peer_info.state.clone(),
                    counter: peer_info.counter,
                };
                peers_info.push(encoded_peer_info);
            }
        }

        postcard::to_allocvec(&peers_info).unwrap()
    }

    pub fn encode_all(&self) -> Vec<u8> {
        let mut peers_info = Vec::new();
        let now = get_sys_timestamp() as Timestamp;
        for (peer, peer_info) in self.peers.iter() {
            if now - peer_info.timestamp > self.timeout {
                continue;
            }

            let encoded_peer_info = EncodedPeerInfo {
                peer: *peer,
                record: peer_info.state.clone(),
                counter: peer_info.counter,
            };
            peers_info.push(encoded_peer_info);
        }

        postcard::to_allocvec(&peers_info).unwrap()
    }

    /// Returns (updated, added)
    pub fn apply(&mut self, encoded_peers_info: &[u8]) -> (Vec<PeerID>, Vec<PeerID>) {
        let peers_info: Vec<EncodedPeerInfo> = postcard::from_bytes(encoded_peers_info).unwrap();
        let mut changed_peers = Vec::new();
        let mut added_peers = Vec::new();
        let now = get_sys_timestamp() as Timestamp;
        for peer_info in peers_info {
            match self.peers.get(&peer_info.peer) {
                Some(x) if x.counter >= peer_info.counter || peer_info.peer == self.peer => {
                    // do nothing
                }
                _ => {
                    let old = self.peers.insert(
                        peer_info.peer,
                        PeerInfo {
                            counter: peer_info.counter,
                            state: peer_info.record,
                            timestamp: now,
                        },
                    );
                    if old.is_some() {
                        changed_peers.push(peer_info.peer);
                    } else {
                        added_peers.push(peer_info.peer);
                    }
                }
            }
        }

        (changed_peers, added_peers)
    }

    pub fn set_local_state(&mut self, value: impl Into<LoroValue>) {
        self._set_local_state(value.into());
    }

    fn _set_local_state(&mut self, value: LoroValue) {
        let peer = self.peers.entry(self.peer).or_insert_with(|| PeerInfo {
            state: Default::default(),
            counter: 0,
            timestamp: 0,
        });

        peer.state = value;
        peer.counter += 1;
        peer.timestamp = get_sys_timestamp() as Timestamp;
    }

    pub fn get_local_state(&self) -> Option<LoroValue> {
        self.peers.get(&self.peer).map(|x| x.state.clone())
    }

    pub fn remove_outdated(&mut self) -> Vec<PeerID> {
        let now = get_sys_timestamp() as Timestamp;
        let mut removed = Vec::new();
        self.peers.retain(|id, v| {
            if now - v.timestamp > self.timeout {
                removed.push(*id);
                false
            } else {
                true
            }
        });

        removed
    }

    pub fn get_all_states(&self) -> &FxHashMap<PeerID, PeerInfo> {
        &self.peers
    }

    pub fn peer(&self) -> PeerID {
        self.peer
    }
}

pub type LocalAwarenessCallback = Box<dyn Fn(&Vec<u8>) -> bool + Send + Sync + 'static>;

/// `EphemeralStore` is a structure that tracks the ephemeral state of peers.
///
/// It can be used to synchronize cursor positions, selections, and the names of the peers.
/// We use the latest timestamp as the tie-breaker for LWW (Last-Write-Wins) conflict resolution.
pub struct EphemeralStore {
    states: FxHashMap<String, State>,
    subs: SubscriberSetWithQueue<(), LocalAwarenessCallback, Vec<u8>>,
    timeout: i64,
}

impl std::fmt::Debug for EphemeralStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "AwarenessV2 {{ states: {:?}, timeout: {:?} }}",
            self.states, self.timeout
        )
    }
}

#[derive(Serialize, Deserialize)]
struct EncodedState<'a> {
    #[serde(borrow)]
    key: &'a str,
    value: Option<LoroValue>,
    timestamp: i64,
}

#[derive(Debug, Clone)]
struct State {
    state: Option<LoroValue>,
    timestamp: i64,
}

#[derive(Debug, Clone)]
pub struct AwarenessUpdates {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub removed: Vec<String>,
}

impl EphemeralStore {
    pub fn new(timeout: i64) -> EphemeralStore {
        EphemeralStore {
            timeout,
            states: FxHashMap::default(),
            subs: SubscriberSetWithQueue::new(),
        }
    }

    pub fn encode(&self, key: &str) -> Vec<u8> {
        let mut peers_info = Vec::new();
        let now = get_sys_timestamp() as Timestamp;
        if let Some(peer_state) = self.states.get(key) {
            if now - peer_state.timestamp > self.timeout {
                return vec![];
            }
            let encoded_peer_info = EncodedState {
                key,
                value: peer_state.state.clone(),
                timestamp: peer_state.timestamp,
            };
            peers_info.push(encoded_peer_info);
        }

        postcard::to_allocvec(&peers_info).unwrap()
    }

    pub fn encode_all(&self) -> Vec<u8> {
        let mut peers_info = Vec::new();
        let now = get_sys_timestamp() as Timestamp;
        for (key, peer_state) in self.states.iter() {
            if now - peer_state.timestamp > self.timeout {
                continue;
            }
            let encoded_peer_info = EncodedState {
                key,
                value: peer_state.state.clone(),
                timestamp: peer_state.timestamp,
            };
            peers_info.push(encoded_peer_info);
        }
        postcard::to_allocvec(&peers_info).unwrap()
    }

    /// Returns (updated, added, removed)
    pub fn apply(&mut self, data: &[u8]) -> AwarenessUpdates {
        let peers_info: Vec<EncodedState> = postcard::from_bytes(data).unwrap();
        let mut updated_keys = Vec::new();
        let mut added_keys = Vec::new();
        let mut removed_keys = Vec::new();
        let now = get_sys_timestamp() as Timestamp;
        for EncodedState {
            key,
            value: record,
            timestamp,
        } in peers_info
        {
            match self.states.get_mut(key) {
                Some(peer_info) if peer_info.timestamp >= timestamp => {
                    // do nothing
                }
                _ => {
                    let old = self.states.insert(
                        key.to_string(),
                        State {
                            state: record.clone(),
                            timestamp: now,
                        },
                    );
                    match (old, record) {
                        (Some(_), Some(_)) => updated_keys.push(key.to_string()),
                        (None, Some(_)) => added_keys.push(key.to_string()),
                        (Some(_), None) => removed_keys.push(key.to_string()),
                        (None, None) => {}
                    }
                }
            }
        }

        AwarenessUpdates {
            added: added_keys,
            updated: updated_keys,
            removed: removed_keys,
        }
    }

    pub fn set(&mut self, key: &str, value: impl Into<LoroValue>) {
        self._set_local_state(key, Some(value.into()));
    }

    pub fn delete(&mut self, key: &str) {
        self._set_local_state(key, None);
    }

    pub fn get(&self, key: &str) -> Option<LoroValue> {
        self.states.get(key).and_then(|x| x.state.clone())
    }

    pub fn remove_outdated(&mut self) -> Vec<String> {
        let now = get_sys_timestamp() as Timestamp;
        let mut removed = Vec::new();

        self.states.retain(|key, state| {
            if now - state.timestamp > self.timeout {
                if state.state.is_some() {
                    removed.push(key.clone());
                }
                false
            } else {
                true
            }
        });

        removed
    }

    pub fn get_all_states(&self) -> FxHashMap<String, LoroValue> {
        self.states
            .iter()
            .filter(|(_, v)| v.state.is_some())
            .map(|(k, v)| (k.clone(), v.state.clone().unwrap()))
            .collect()
    }

    pub fn subscribe_local_update(&self, callback: LocalAwarenessCallback) -> Subscription {
        let (sub, activate) = self.subs.inner().insert((), callback);
        activate();
        sub
    }

    fn _set_local_state(&mut self, key: &str, value: Option<LoroValue>) {
        self.states.insert(
            key.to_string(),
            State {
                state: value,
                timestamp: get_sys_timestamp() as Timestamp,
            },
        );
        if self.subs.inner().is_empty() {
            return;
        }
        self.subs.emit(&(), self.encode(key));
    }
}
