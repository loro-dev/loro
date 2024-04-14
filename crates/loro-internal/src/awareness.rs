use fxhash::FxHashMap;
use loro_common::{LoroValue, PeerID};
use serde::{Deserialize, Serialize};

use crate::change::get_sys_timestamp;

#[derive(Debug, Clone)]
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
        let now = get_sys_timestamp();
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

    /// Returnes (updated, added)
    pub fn apply(&mut self, encoded_peers_info: &[u8]) -> (Vec<PeerID>, Vec<PeerID>) {
        let peers_info: Vec<EncodedPeerInfo> = postcard::from_bytes(encoded_peers_info).unwrap();
        let mut changed_peers = Vec::new();
        let mut added_peers = Vec::new();
        let now = get_sys_timestamp();
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

    pub fn set_local_state(&mut self, value: LoroValue) {
        let peer = self.peers.entry(self.peer).or_insert_with(|| PeerInfo {
            state: Default::default(),
            counter: 0,
            timestamp: 0,
        });

        peer.state = value;
        peer.counter += 1;
        peer.timestamp = get_sys_timestamp();
    }

    pub fn remove_outdated(&mut self) -> Vec<PeerID> {
        let now = get_sys_timestamp();
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
