use fxhash::FxHashMap;
use loro_common::{LoroValue, PeerID};
use serde::{Deserialize, Serialize};

use crate::change::{get_sys_timestamp, Timestamp};

/// `Awareness` is a structure that tracks the ephemeral state of peers.
///
/// It can be used to synchronize cursor positions, selections, and the names of the peers.
///
/// The state of a specific peer is expected to be removed after a specified timeout. Use
/// `remove_outdated` to eliminate outdated states.
#[derive(Debug, Clone)]
#[deprecated(since = "1.4.6", note = "Use `AwarenessV2` instead.")]
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

pub mod v2 {
    use fxhash::{FxHashMap, FxHashSet};
    use loro_common::{LoroValue, PeerID};
    use serde::{Deserialize, Serialize};

    use crate::change::{get_sys_timestamp, Timestamp};

    #[derive(Debug, Clone)]
    pub struct AwarenessV2 {
        peer: PeerID,
        peers: FxHashMap<PeerID, FxHashMap<String, PeerInfo>>,
        timeout: i64,
    }

    #[derive(Serialize, Deserialize)]
    struct EncodedPeerInfo<'a> {
        peer: PeerID,
        #[serde(borrow)]
        field: &'a str,
        record: LoroValue,
        timestamp: i64,
    }

    #[derive(Debug, Clone)]
    pub struct PeerInfo {
        pub state: LoroValue,
        // This field is generated locally
        pub timestamp: i64,
    }

    impl AwarenessV2 {
        pub fn new(peer: PeerID, timeout: i64) -> AwarenessV2 {
            AwarenessV2 {
                peer,
                timeout,
                peers: FxHashMap::default(),
            }
        }

        pub fn encode(&self, peers: &[PeerID], field: &str) -> Vec<u8> {
            let mut peers_info = Vec::new();
            let now = get_sys_timestamp() as Timestamp;
            for peer in peers {
                if let Some(peer_state) = self.peers.get(peer) {
                    let Some(peer_info) = peer_state.get(field) else {
                        continue;
                    };
                    if now - peer_info.timestamp > self.timeout {
                        continue;
                    }
                    let encoded_peer_info = EncodedPeerInfo {
                        peer: *peer,
                        field,
                        record: peer_info.state.clone(),
                        timestamp: peer_info.timestamp,
                    };
                    peers_info.push(encoded_peer_info);
                }
            }

            postcard::to_allocvec(&peers_info).unwrap()
        }

        pub fn encode_all(&self) -> Vec<u8> {
            let mut peers_info = Vec::new();
            let now = get_sys_timestamp() as Timestamp;
            for peer in self.peers.keys() {
                if let Some(peer_state) = self.peers.get(peer) {
                    for (field, peer_info) in peer_state.iter() {
                        if now - peer_info.timestamp > self.timeout {
                            continue;
                        }
                        let encoded_peer_info = EncodedPeerInfo {
                            peer: *peer,
                            field,
                            record: peer_info.state.clone(),
                            timestamp: peer_info.timestamp,
                        };
                        peers_info.push(encoded_peer_info);
                    }
                }
            }
            postcard::to_allocvec(&peers_info).unwrap()
        }

        pub fn encode_all_peers(&self, field: &str) -> Vec<u8> {
            self.encode(&self.peers.keys().copied().collect::<Vec<_>>(), field)
        }

        /// Returns (updated, added)
        pub fn apply(
            &mut self,
            encoded_peers_info: &[u8],
        ) -> (FxHashSet<PeerID>, FxHashSet<PeerID>) {
            let peers_info: Vec<EncodedPeerInfo> =
                postcard::from_bytes(encoded_peers_info).unwrap();
            let mut changed_peers = FxHashSet::default();
            let mut added_peers = FxHashSet::default();
            let now = get_sys_timestamp() as Timestamp;
            for EncodedPeerInfo {
                peer,
                field,
                record,
                timestamp,
            } in peers_info
            {
                let peer_state = self.peers.entry(peer).or_insert_with(|| {
                    added_peers.insert(peer);
                    FxHashMap::default()
                });
                match peer_state.get_mut(field) {
                    Some(peer_info) if peer_info.timestamp >= timestamp || peer == self.peer => {
                        // do nothing
                    }
                    _ => {
                        if timestamp < 0 {
                            peer_state.remove(field);
                        } else {
                            peer_state.insert(
                                field.to_string(),
                                PeerInfo {
                                    state: record,
                                    timestamp: now,
                                },
                            );
                        }
                        if !added_peers.contains(&peer) {
                            changed_peers.insert(peer);
                        }
                    }
                }
            }

            (changed_peers, added_peers)
        }

        pub fn set_local_state(&mut self, field: &str, value: impl Into<LoroValue>) {
            self._set_local_state(field, value.into(), false);
        }

        pub fn delete_local_state(&mut self, field: &str) {
            self._set_local_state(field, LoroValue::Null, true);
        }

        fn _set_local_state(&mut self, field: &str, value: LoroValue, delete: bool) {
            let peer = self.peers.entry(self.peer).or_default();
            let peer = peer.entry(field.to_string()).or_insert_with(|| PeerInfo {
                state: Default::default(),
                timestamp: 0,
            });

            peer.state = value;
            peer.timestamp = if delete {
                -(get_sys_timestamp() as Timestamp)
            } else {
                get_sys_timestamp() as Timestamp
            };
        }

        pub fn get_local_state(&self, field: &str) -> Option<LoroValue> {
            self.peers
                .get(&self.peer)
                .and_then(|x| x.get(field))
                .map(|x| x.state.clone())
        }

        pub fn remove_outdated(&mut self, field: &str) -> FxHashSet<PeerID> {
            let now = get_sys_timestamp() as Timestamp;
            let mut removed = FxHashSet::default();
            for (id, v) in self.peers.iter_mut() {
                if let Some(timestamp) = v.get(field).map(|x| x.timestamp) {
                    if now - timestamp > self.timeout {
                        removed.insert(*id);
                        v.remove(field);
                    }
                }
            }

            removed
        }

        pub fn get_all_states(&self, field: &str) -> FxHashMap<PeerID, LoroValue> {
            let mut ans = FxHashMap::default();
            for (id, v) in self.peers.iter() {
                if let Some(peer_info) = v.get(field) {
                    ans.insert(*id, peer_info.state.clone());
                }
            }
            ans
        }

        pub fn peer(&self) -> PeerID {
            self.peer
        }
    }
}
