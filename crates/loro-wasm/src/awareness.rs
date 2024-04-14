use js_sys::{Array, Object, Reflect};
use loro_internal::{awareness::Awareness as InternalAwareness, id::PeerID};
use wasm_bindgen::prelude::*;

use crate::{js_peer_to_peer, JsIntoPeerID, JsResult, JsStrPeerID};

#[wasm_bindgen]
pub struct AwarenessWasm {
    inner: InternalAwareness,
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "{[peer in PeerID]: unknown}")]
    pub type JsAwarenessStates;
    #[wasm_bindgen(typescript_type = "{ updated: PeerID[], added: PeerID[] }")]
    pub type JsAwarenessApplyResult;
}

#[wasm_bindgen]
impl AwarenessWasm {
    #[wasm_bindgen(constructor)]
    pub fn new(peer: JsIntoPeerID, timeout: f64) -> AwarenessWasm {
        let id = js_peer_to_peer(peer.into()).unwrap_throw();
        AwarenessWasm {
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

    pub fn encodeAll(&self) -> Vec<u8> {
        self.inner.encode_all()
    }

    pub fn apply(&mut self, encoded_peers_info: Vec<u8>) -> JsResult<JsAwarenessApplyResult> {
        let (updated, added) = self.inner.apply(&encoded_peers_info);
        let ans = Object::new();
        let updated = Array::from_iter(updated.into_iter().map(peer_to_str_js));
        let added = Array::from_iter(added.into_iter().map(peer_to_str_js));
        Reflect::set(&ans, &"updated".into(), &updated.into())?;
        Reflect::set(&ans, &"added".into(), &added.into())?;
        let v: JsValue = ans.into();
        Ok(v.into())
    }

    #[wasm_bindgen(skip_typescript)]
    pub fn setLocalState(&mut self, value: JsValue) {
        self.inner.set_local_state(value);
    }

    pub fn peer(&self) -> JsStrPeerID {
        let v: JsValue = format!("{}", self.inner.peer()).into();
        v.into()
    }

    #[wasm_bindgen(skip_typescript)]
    pub fn getAllStates(&self) -> JsAwarenessStates {
        let states = self.inner.get_all_states();
        let obj = Object::new();
        for (peer, state) in states {
            Reflect::set(&obj, &peer_to_str_js(*peer), &state.state.clone().into()).unwrap();
        }
        let v: JsValue = obj.into();
        v.into()
    }

    #[wasm_bindgen(skip_typescript)]
    pub fn getState(&self, peer: JsIntoPeerID) -> JsValue {
        let id = js_peer_to_peer(peer.into()).unwrap_throw();
        let Some(state) = self.inner.get_all_states().get(&id) else {
            return JsValue::UNDEFINED;
        };

        state.state.clone().into()
    }

    pub fn getTimestamp(&self, peer: JsIntoPeerID) -> Option<f64> {
        let id = js_peer_to_peer(peer.into()).unwrap_throw();
        self.inner
            .get_all_states()
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

    pub fn length(&self) -> i32 {
        self.inner.get_all_states().len() as i32
    }

    pub fn isEmpty(&self) -> bool {
        self.inner.get_all_states().is_empty()
    }

    pub fn peers(&self) -> Vec<JsStrPeerID> {
        self.inner
            .get_all_states()
            .keys()
            .map(|id| {
                let v: JsValue = peer_to_str_js(*id);
                v.into()
            })
            .collect()
    }
}

fn peer_to_str_js(peer: PeerID) -> JsValue {
    format!("{}", peer).into()
}
