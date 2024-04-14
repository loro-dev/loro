use js_sys::{Array, Object, Reflect};
use loro_internal::{awareness::Awareness as InternalAwareness, id::PeerID, FxHashMap, LoroValue};
use wasm_bindgen::prelude::*;

use crate::{js_peer_to_peer, JsIntoPeerID, JsStrPeerID};

#[wasm_bindgen]
pub struct Awareness {
    inner: InternalAwareness,
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "{[peer in PeerID]: Record<string, unknown>}")]
    pub type JsAwarenessRecord;
}

#[wasm_bindgen]
impl Awareness {
    #[wasm_bindgen(constructor)]
    pub fn new(peer: JsIntoPeerID, timeout: f64) -> Awareness {
        let id = js_peer_to_peer(peer.into()).unwrap_throw();
        Awareness {
            inner: InternalAwareness::new(id, timeout as i64),
        }
    }

    pub fn encode(&self, peers: Array) -> Vec<u8> {
        let mut peer_vec = Vec::with_capacity(peers.length() as usize);
        for peer in peers.iter() {
            peer_vec.push(js_peer_to_peer(peer).unwrap_throw());
        }

        self.inner.encode(&peer_vec)
    }

    pub fn apply(&mut self, encoded_peers_info: Vec<u8>) -> Vec<JsStrPeerID> {
        let peers = self.inner.apply(&encoded_peers_info);
        peers
            .into_iter()
            .map(|id| {
                let v: JsValue = peer_to_str_js(id);
                v.into()
            })
            .collect()
    }

    pub fn setLocalRecord(&mut self, key: String, value: JsValue) {
        self.inner.set_record(key, value.into());
    }

    pub fn peer(&self) -> JsStrPeerID {
        let v: JsValue = format!("{}", self.inner.peer()).into();
        v.into()
    }

    pub fn getAllRecords(&self) -> JsAwarenessRecord {
        let records = self.inner.get_all_records();
        let obj = Object::new();
        for (peer, record) in records {
            Reflect::set(
                &obj,
                &peer_to_str_js(*peer),
                &peer_info_to_js(&record.record),
            )
            .unwrap();
        }
        let v: JsValue = obj.into();
        v.into()
    }

    pub fn getRecord(&self, peer: JsIntoPeerID) -> JsValue {
        let id = js_peer_to_peer(peer.into()).unwrap_throw();
        let Some(record) = self.inner.get_all_records().get(&id) else {
            return JsValue::UNDEFINED;
        };
        peer_info_to_js(&record.record)
    }

    pub fn getTimestamp(&self, peer: JsIntoPeerID) -> Option<f64> {
        let id = js_peer_to_peer(peer.into()).unwrap_throw();
        self.inner
            .get_all_records()
            .get(&id)
            .map(|r| r.timestamp as f64)
    }

    pub fn removeOutdated(&mut self) -> Vec<JsStrPeerID> {
        let outdated = self.inner.remove_outdated();
        outdated
            .into_iter()
            .map(|id| {
                let v: JsValue = peer_to_str_js(id);
                v.into()
            })
            .collect()
    }
}

fn peer_to_str_js(peer: PeerID) -> JsValue {
    format!("{}", peer).into()
}

fn peer_info_to_js(peer_info: &FxHashMap<String, LoroValue>) -> JsValue {
    let obj = Object::new();
    for (key, value) in peer_info {
        Reflect::set(&obj, &key.into(), &value.clone().into()).unwrap();
    }
    obj.into()
}
