use std::sync::atomic::AtomicI64;
use std::sync::{Arc, Mutex};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EphemeralEventTrigger {
    Local,
    Import,
    Timeout,
}

#[derive(Debug, Clone)]
pub struct EphemeralStoreEvent {
    pub by: EphemeralEventTrigger,
    pub added: Arc<Vec<String>>,
    pub updated: Arc<Vec<String>>,
    pub removed: Arc<Vec<String>>,
}

pub type LocalEphemeralCallback = Box<dyn Fn(&Vec<u8>) -> bool + Send + Sync + 'static>;
pub type EphemeralSubscriber = Box<dyn Fn(&EphemeralStoreEvent) -> bool + Send + Sync + 'static>;

/// `EphemeralStore` is a structure that tracks ephemeral key-value state across peers.
///
/// - Use it for syncing lightweight presence/state like cursors, selections, and UI hints.
/// - Each key uses timestamp-based LWW (Last-Write-Wins) conflict resolution.
/// - Timeout unit: milliseconds.
/// - After timeout: entries are considered expired. They are omitted from
///   `encode`/`encode_all`, and calling [`remove_outdated`] will purge them and
///   notify subscribers with `EphemeralEventTrigger::Timeout`.
///
/// In Rust, you are responsible for periodically calling [`remove_outdated`]
/// if you want timed-out entries to be removed and corresponding events to be
/// emitted. In the WASM/TS wrapper, a timer runs automatically while the store
/// is non-empty.
///
/// See: https://loro.dev/docs/tutorial/ephemeral
///
/// # Example
///
/// ```rust
/// use loro_internal::awareness::EphemeralStore;
///
/// let mut store = EphemeralStore::new(1000);
/// store.set("key", "value");
/// let encoded = store.encode("key");
/// let mut store2 = EphemeralStore::new(1000);
/// store.subscribe_local_updates(Box::new(|data| {
///     println!("local update: {:?}", data);
///     true
/// }));
/// store2.apply(&encoded);
/// assert_eq!(store2.get("key"), Some("value".into()));
/// ```
#[derive(Debug, Clone)]
pub struct EphemeralStore {
    inner: Arc<EphemeralStoreInner>,
}

impl EphemeralStore {
    /// Create a new `EphemeralStore`.
    ///
    /// - `timeout`: inactivity timeout in milliseconds. If a key does not
    ///   receive updates within this duration, it is considered expired. It
    ///   will be skipped by `encode`/`encode_all`, and removed when
    ///   [`remove_outdated`] is called (triggering a `Timeout` event to
    ///   subscribers).
    pub fn new(timeout: i64) -> Self {
        Self {
            inner: Arc::new(EphemeralStoreInner::new(timeout)),
        }
    }

    /// Encode the latest value of `key`.
    ///
    /// Expired keys (past timeout) are omitted and produce an empty payload.
    pub fn encode(&self, key: &str) -> Vec<u8> {
        self.inner.encode(key)
    }

    /// Encode all non-expired keys.
    ///
    /// Entries that have exceeded the timeout are not included in the payload.
    pub fn encode_all(&self) -> Vec<u8> {
        self.inner.encode_all()
    }
    
    /// Apply encoded updates imported from another peer/process.
    ///
    /// Subscribers receive an event with `by = Import` and the lists of
    /// `added`/`updated`/`removed` keys as appropriate.
    pub fn apply(&self, data: &[u8]) -> Result<(), Box<str>> {
        self.inner.apply(data)
    }

    pub fn set(&self, key: &str, value: impl Into<LoroValue>) {
        self.inner.set(key, value)
    }

    pub fn delete(&self, key: &str) {
        self.inner.delete(key)
    }

    pub fn get(&self, key: &str) -> Option<LoroValue> {
        self.inner.get(key)
    }

    /// Remove entries whose last update timestamp has exceeded the timeout.
    ///
    /// If any keys are removed, subscribers receive an event with
    /// `by = Timeout` and the `removed` keys. This does not run automatically
    /// in Rust; call it on your own schedule.
    pub fn remove_outdated(&self) {
        self.inner.remove_outdated()
    }

    pub fn get_all_states(&self) -> FxHashMap<String, LoroValue> {
        self.inner.get_all_states()
    }

    pub fn keys(&self) -> Vec<String> {
        self.inner.keys()
    }

    /// Subscribe to local ephemeral/awareness updates.
    ///
    /// The callback receives encoded update bytes whenever local ephemeral state changes.
    /// This is useful for syncing awareness information like cursor positions, selections,
    /// or presence data to other peers in real-time.
    ///
    /// **Auto-unsubscription**: If the callback returns `false`, the subscription will be
    /// automatically removed, providing a convenient way to implement one-time or conditional
    /// subscriptions in Rust.
    ///
    /// # Parameters
    /// - `callback`: Function that receives `&Vec<u8>` (encoded ephemeral updates) and returns `bool`
    ///   - Return `true` to keep the subscription active
    ///   - Return `false` to automatically unsubscribe
    ///
    /// # Example
    /// ```rust
    /// use loro_internal::awareness::EphemeralStore;
    /// use std::sync::{Arc, Mutex};
    ///
    /// let store = EphemeralStore::new(30000);
    /// let update_count = Arc::new(Mutex::new(0));
    /// let count_clone = update_count.clone();
    ///
    /// // Subscribe and collect first 3 updates, then auto-unsubscribe
    /// let sub = store.subscribe_local_updates(Box::new(move |bytes| {
    ///     println!("Received {} bytes of ephemeral data", bytes.len());
    ///     let mut count = count_clone.lock().unwrap();
    ///     *count += 1;
    ///     *count < 3  // Auto-unsubscribe after 3 updates
    /// }));
    ///
    /// store.set("cursor", 42);
    /// ```
    pub fn subscribe_local_updates(&self, callback: LocalEphemeralCallback) -> Subscription {
        self.inner.subscribe_local_updates(callback)
    }

    pub fn subscribe(&self, callback: EphemeralSubscriber) -> Subscription {
        self.inner.subscribe(callback)
    }
}

struct EphemeralStoreInner {
    states: Mutex<FxHashMap<String, State>>,
    local_subs: SubscriberSetWithQueue<(), LocalEphemeralCallback, Vec<u8>>,
    subscribers: SubscriberSetWithQueue<(), EphemeralSubscriber, EphemeralStoreEvent>,
    timeout: AtomicI64,
}

impl std::fmt::Debug for EphemeralStoreInner {
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

impl EphemeralStoreInner {
    pub fn new(timeout: i64) -> EphemeralStoreInner {
        EphemeralStoreInner {
            timeout: AtomicI64::new(timeout),
            states: Mutex::new(FxHashMap::default()),
            local_subs: SubscriberSetWithQueue::new(),
            subscribers: SubscriberSetWithQueue::new(),
        }
    }

    pub fn encode(&self, key: &str) -> Vec<u8> {
        let mut peers_info = Vec::new();
        let now = get_sys_timestamp() as Timestamp;
        let states = self.states.lock().unwrap();
        if let Some(peer_state) = states.get(key) {
            if now - peer_state.timestamp > self.timeout.load(std::sync::atomic::Ordering::Relaxed)
            {
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
        let states = self.states.lock().unwrap();
        for (key, peer_state) in states.iter() {
            if now - peer_state.timestamp > self.timeout.load(std::sync::atomic::Ordering::Relaxed)
            {
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

    pub fn apply(&self, data: &[u8]) -> Result<(), Box<str>> {
        let peers_info = match postcard::from_bytes::<Vec<EncodedState>>(data) {
            Ok(ans) => ans,
            Err(err) => return Err(format!("Failed to decode data: {}", err).into()),
        };

        let mut updated_keys = Vec::new();
        let mut added_keys = Vec::new();
        let mut removed_keys = Vec::new();
        let now = get_sys_timestamp() as Timestamp;
        let mut states = self.states.lock().unwrap();
        for EncodedState {
            key,
            value: record,
            timestamp,
        } in peers_info
        {
            match states.get_mut(key) {
                Some(peer_info) if peer_info.timestamp >= timestamp => {
                    // do nothing
                }
                _ => {
                    let old = states.insert(
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

        drop(states);
        if !self.subscribers.inner().is_empty() {
            self.subscribers.emit(
                &(),
                EphemeralStoreEvent {
                    by: EphemeralEventTrigger::Import,
                    added: Arc::new(added_keys),
                    updated: Arc::new(updated_keys),
                    removed: Arc::new(removed_keys),
                },
            );
        }

        Ok(())
    }

    pub fn set(&self, key: &str, value: impl Into<LoroValue>) {
        self._set_local_state(key, Some(value.into()));
    }

    pub fn delete(&self, key: &str) {
        self._set_local_state(key, None);
    }

    pub fn get(&self, key: &str) -> Option<LoroValue> {
        let states = self.states.lock().unwrap();
        states.get(key).and_then(|x| x.state.clone())
    }

    pub fn remove_outdated(&self) {
        let now = get_sys_timestamp() as Timestamp;
        let mut removed = Vec::new();
        let mut states = self.states.lock().unwrap();
        states.retain(|key, state| {
            if now - state.timestamp > self.timeout.load(std::sync::atomic::Ordering::Relaxed) {
                if state.state.is_some() {
                    removed.push(key.clone());
                }
                false
            } else {
                true
            }
        });
        drop(states);
        if !self.subscribers.inner().is_empty() {
            self.subscribers.emit(
                &(),
                EphemeralStoreEvent {
                    by: EphemeralEventTrigger::Timeout,
                    added: Arc::new(Vec::new()),
                    updated: Arc::new(Vec::new()),
                    removed: Arc::new(removed),
                },
            );
        }
    }

    pub fn get_all_states(&self) -> FxHashMap<String, LoroValue> {
        let states = self.states.lock().unwrap();
        states
            .iter()
            .filter(|(_, v)| v.state.is_some())
            .map(|(k, v)| (k.clone(), v.state.clone().unwrap()))
            .collect()
    }

    pub fn keys(&self) -> Vec<String> {
        let states = self.states.lock().unwrap();
        states
            .keys()
            .filter(|&k| states.get(k).unwrap().state.is_some())
            .map(|s| s.to_string())
            .collect()
    }

    /// Subscribe to local ephemeral state updates.
    ///
    /// The callback receives encoded update bytes whenever the local peer's ephemeral state
    /// is modified. This enables real-time synchronization of awareness data like cursor
    /// positions, user presence, or temporary collaborative state.
    ///
    /// **Auto-unsubscription**: If the callback returns `false`, the subscription will be
    /// automatically removed, making it easy to implement conditional or temporary subscriptions.
    ///
    /// # Parameters
    /// - `callback`: Function that receives `&Vec<u8>` and returns `bool`
    ///   - Return `true` to maintain the subscription
    ///   - Return `false` to automatically unsubscribe
    ///
    /// # Returns
    /// A `Subscription` that can be used to manually unsubscribe if needed.
    pub fn subscribe_local_updates(&self, callback: LocalEphemeralCallback) -> Subscription {
        let (sub, activate) = self.local_subs.inner().insert((), callback);
        activate();
        sub
    }

    pub fn subscribe(&self, callback: EphemeralSubscriber) -> Subscription {
        let (sub, activate) = self.subscribers.inner().insert((), callback);
        activate();
        sub
    }

    fn _set_local_state(&self, key: &str, value: Option<LoroValue>) {
        let is_delete = value.is_none();
        let mut states = self.states.lock().unwrap();
        let old = states.insert(
            key.to_string(),
            State {
                state: value,
                timestamp: get_sys_timestamp() as Timestamp,
            },
        );

        drop(states);
        if !self.local_subs.inner().is_empty() {
            self.local_subs.emit(&(), self.encode(key));
        }
        if !self.subscribers.inner().is_empty() {
            if old.is_some() {
                self.subscribers.emit(
                    &(),
                    EphemeralStoreEvent {
                        by: EphemeralEventTrigger::Local,
                        added: Arc::new(Vec::new()),
                        updated: if !is_delete {
                            Arc::new(vec![key.to_string()])
                        } else {
                            Arc::new(Vec::new())
                        },
                        removed: if !is_delete {
                            Arc::new(Vec::new())
                        } else {
                            Arc::new(vec![key.to_string()])
                        },
                    },
                );
            } else if !is_delete {
                self.subscribers.emit(
                    &(),
                    EphemeralStoreEvent {
                        by: EphemeralEventTrigger::Local,
                        added: Arc::new(vec![key.to_string()]),
                        updated: Arc::new(Vec::new()),
                        removed: Arc::new(Vec::new()),
                    },
                );
            }
        }
    }
}
