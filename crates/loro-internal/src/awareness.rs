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
    pub record: FxHashMap<String, LoroValue>,
    // This field is generated locally
    pub timestamp: i64,
}

#[derive(Serialize, Deserialize)]
struct EncodedPeerInfo {
    peer: PeerID,
    record: FxHashMap<String, LoroValue>,
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
                    record: peer_info.record.clone(),
                };
                peers_info.push(encoded_peer_info);
            }
        }

        postcard::to_allocvec(&peers_info).unwrap()
    }

    pub fn apply(&mut self, encoded_peers_info: &[u8]) -> Vec<PeerID> {
        let peers_info: Vec<EncodedPeerInfo> = postcard::from_bytes(encoded_peers_info).unwrap();
        let changed_peers = peers_info
            .iter()
            .map(|peer_info| peer_info.peer)
            .collect::<Vec<_>>();
        let now = get_sys_timestamp();
        for peer_info in peers_info {
            self.peers.insert(
                peer_info.peer,
                PeerInfo {
                    record: peer_info.record,
                    timestamp: now,
                },
            );
        }

        changed_peers
    }

    pub fn set_record(&mut self, key: String, value: LoroValue) {
        let peer = self.peers.entry(self.peer).or_insert_with(|| PeerInfo {
            record: Default::default(),
            timestamp: 0,
        });

        peer.record.insert(key, value);
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

    pub fn get_all_records(&self) -> &FxHashMap<PeerID, PeerInfo> {
        &self.peers
    }

    pub fn peer(&self) -> PeerID {
        self.peer
    }
}
