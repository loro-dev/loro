#![allow(deprecated)]
use js_sys::{Array, Object, Reflect};
use loro_internal::{
    awareness::{
        Awareness as InternalAwareness, EphemeralEventTrigger,
        EphemeralStore as InternalEphemeralStore, EphemeralStoreEvent,
    },
    id::PeerID,
};
use wasm_bindgen::prelude::*;

use crate::{
    console_error, js_peer_to_peer, observer, subscription_to_js_function_callback, JsIntoPeerID,
    JsResult, JsStrPeerID,
};

/// `Awareness` is a structure that tracks the ephemeral state of peers.
///
/// It can be used to synchronize cursor positions, selections, and the names of the peers.
///
/// The state of a specific peer is expected to be removed after a specified timeout. Use
/// `remove_outdated` to eliminate outdated states.
#[wasm_bindgen]
pub struct AwarenessWasm {
    inner: InternalAwareness,
}

#[wasm_bindgen]
extern "C" {
    /// Awareness states
    #[wasm_bindgen(typescript_type = "{[peer in PeerID]: unknown}")]
    pub type JsAwarenessStates;
    /// Awareness apply result
    #[wasm_bindgen(typescript_type = "{ updated: PeerID[], added: PeerID[] }")]
    pub type JsAwarenessApplyResult;
    /// Awareness updates
    #[wasm_bindgen(typescript_type = "{ updated: string[], added: string[], removed: string[] }")]
    pub type JsAwarenessUpdates;
}

#[wasm_bindgen]
impl AwarenessWasm {
    /// Creates a new `Awareness` instance.
    ///
    /// The `timeout` parameter specifies the duration in milliseconds.
    /// A state of a peer is considered outdated, if the last update of the state of the peer
    /// is older than the `timeout`.
    #[wasm_bindgen(constructor)]
    pub fn new(peer: JsIntoPeerID, timeout: f64) -> AwarenessWasm {
        let id = js_peer_to_peer(peer.into()).unwrap_throw();
        AwarenessWasm {
            inner: InternalAwareness::new(id, timeout as i64),
        }
    }

    /// Encodes the state of the given peers.
    pub fn encode(&self, peers: Array) -> Vec<u8> {
        let mut peer_vec = Vec::with_capacity(peers.length() as usize);
        for peer in peers.iter() {
            peer_vec.push(js_peer_to_peer(peer).unwrap_throw());
        }

        self.inner.encode(&peer_vec)
    }

    /// Encodes the state of all peers.
    pub fn encodeAll(&self) -> Vec<u8> {
        self.inner.encode_all()
    }

    /// Applies the encoded state of peers.
    ///
    /// Each peer's deletion countdown will be reset upon update, requiring them to pass through the `timeout`
    /// interval again before being eligible for deletion.
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

    /// Sets the state of the local peer.
    #[wasm_bindgen(skip_typescript)]
    pub fn setLocalState(&mut self, value: JsValue) {
        self.inner.set_local_state(value);
    }

    /// Get the PeerID of the local peer.
    pub fn peer(&self) -> JsStrPeerID {
        let v: JsValue = format!("{}", self.inner.peer()).into();
        v.into()
    }

    /// Get the state of all peers.
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

    /// Get the state of a given peer.
    #[wasm_bindgen(skip_typescript)]
    pub fn getState(&self, peer: JsIntoPeerID) -> JsValue {
        let id = js_peer_to_peer(peer.into()).unwrap_throw();
        let Some(state) = self.inner.get_all_states().get(&id) else {
            return JsValue::UNDEFINED;
        };

        state.state.clone().into()
    }

    /// Get the timestamp of the state of a given peer.
    pub fn getTimestamp(&self, peer: JsIntoPeerID) -> Option<f64> {
        let id = js_peer_to_peer(peer.into()).unwrap_throw();
        self.inner
            .get_all_states()
            .get(&id)
            .map(|r| r.timestamp as f64)
    }

    /// Remove the states of outdated peers.
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

    /// Get the number of peers.
    pub fn length(&self) -> i32 {
        self.inner.get_all_states().len() as i32
    }

    /// If the state is empty.
    pub fn isEmpty(&self) -> bool {
        self.inner.get_all_states().is_empty()
    }

    /// Get all the peers
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

#[wasm_bindgen]
pub struct EphemeralStoreWasm {
    inner: InternalEphemeralStore,
}

#[wasm_bindgen]
impl EphemeralStoreWasm {
    /// Creates a new `EphemeralStore` instance.
    ///
    /// The `timeout` parameter specifies the duration in milliseconds.
    /// A state of a peer is considered outdated, if the last update of the state of the peer
    /// is older than the `timeout`.
    #[wasm_bindgen(constructor)]
    pub fn new(timeout: f64) -> EphemeralStoreWasm {
        EphemeralStoreWasm {
            inner: InternalEphemeralStore::new(timeout as i64),
        }
    }

    pub fn set(&self, key: &str, value: JsValue) {
        self.inner.set(key, value);
    }

    pub fn delete(&self, key: &str) {
        self.inner.delete(key);
    }

    pub fn get(&self, key: &str) -> JsValue {
        self.inner
            .get(key)
            .map(|v| v.into())
            .unwrap_or(JsValue::UNDEFINED)
    }

    pub fn getAllStates(&self) -> JsValue {
        let states = self.inner.get_all_states();
        let obj = Object::new();
        for (key, value) in states {
            Reflect::set(&obj, &key.into(), &value.into()).unwrap();
        }
        obj.into()
    }

    #[wasm_bindgen(skip_typescript)]
    pub fn subscribeLocalUpdates(&self, f: js_sys::Function) -> JsValue {
        let observer = observer::Observer::new(f);
        let sub = self.inner.subscribe_local_updates(Box::new(move |e| {
            let arr = js_sys::Uint8Array::new_with_length(e.len() as u32);
            arr.copy_from(e);
            if let Err(e) = observer.call1(&arr.into()) {
                console_error!("EphemeralStore subscribeLocalUpdate: Error: {:?}", e);
            }
            true
        }));

        subscription_to_js_function_callback(sub)
    }

    #[wasm_bindgen(skip_typescript)]
    pub fn subscribe(&self, f: js_sys::Function) -> JsValue {
        let observer = observer::Observer::new(f);
        let sub = self.inner.subscribe(Box::new(
            move |EphemeralStoreEvent {
                      by,
                      added,
                      updated,
                      removed,
                  }| {
                let obj = Object::new();
                Reflect::set(
                    &obj,
                    &"added".into(),
                    &added
                        .iter()
                        .map(|s| JsValue::from_str(s))
                        .collect::<Array>()
                        .into(),
                )
                .unwrap();
                Reflect::set(
                    &obj,
                    &"updated".into(),
                    &updated
                        .iter()
                        .map(|s| JsValue::from_str(s))
                        .collect::<Array>()
                        .into(),
                )
                .unwrap();
                Reflect::set(
                    &obj,
                    &"removed".into(),
                    &removed
                        .iter()
                        .map(|s| JsValue::from_str(s))
                        .collect::<Array>()
                        .into(),
                )
                .unwrap();
                Reflect::set(
                    &obj,
                    &"by".into(),
                    &match by {
                        EphemeralEventTrigger::Local => JsValue::from_str("local"),
                        EphemeralEventTrigger::Import => JsValue::from_str("import"),
                        EphemeralEventTrigger::Timeout => JsValue::from_str("timeout"),
                    },
                )
                .unwrap();
                observer.call1(&obj.into()).unwrap();
                true
            },
        ));

        subscription_to_js_function_callback(sub)
    }

    pub fn encode(&self, key: &str) -> Vec<u8> {
        self.inner.encode(key)
    }

    pub fn encodeAll(&self) -> Vec<u8> {
        self.inner.encode_all()
    }

    pub fn apply(&self, data: &[u8]) {
        self.inner.apply(data);
    }

    pub fn removeOutdated(&self) {
        self.inner.remove_outdated()
    }

    /// If the state is empty.
    pub fn isEmpty(&self) -> bool {
        self.inner.get_all_states().is_empty()
    }

    pub fn keys(&self) -> Vec<String> {
        self.inner.keys()
    }
}
