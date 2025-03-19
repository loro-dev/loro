use crate::{LoroValue, LoroValueLike, Subscription};
use loro::awareness::{EphemeralEventTrigger, EphemeralStore as InternalEphemeralStore};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct EphemeralStoreEvent {
    pub by: EphemeralEventTrigger,
    pub added: Arc<Vec<String>>,
    pub removed: Arc<Vec<String>>,
    pub updated: Arc<Vec<String>>,
}

pub trait LocalEphemeralListener: Sync + Send {
    fn on_ephemeral_update(&self, update: Vec<u8>);
}

pub trait EphemeralSubscriber: Sync + Send {
    fn on_ephemeral_event(&self, event: EphemeralStoreEvent);
}
pub struct EphemeralStore(Mutex<InternalEphemeralStore>);

impl EphemeralStore {
    pub fn new(timeout: i64) -> Self {
        Self(Mutex::new(InternalEphemeralStore::new(timeout)))
    }

    pub fn encode(&self, key: &str) -> Vec<u8> {
        self.0.lock().unwrap().encode(key)
    }

    pub fn encode_all(&self) -> Vec<u8> {
        self.0.lock().unwrap().encode_all()
    }

    pub fn apply(&self, data: &[u8]) {
        self.0.lock().unwrap().apply(data)
    }

    pub fn set(&self, key: &str, value: Arc<dyn LoroValueLike>) {
        self.0.lock().unwrap().set(key, value.as_loro_value())
    }

    pub fn delete(&self, key: &str) {
        self.0.lock().unwrap().delete(key)
    }

    pub fn get(&self, key: &str) -> Option<LoroValue> {
        self.0.lock().unwrap().get(key).map(|v| v.into())
    }

    pub fn remove_outdated(&self) {
        self.0.lock().unwrap().remove_outdated()
    }

    pub fn keys(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .keys()
            .map(|s| s.to_string())
            .collect()
    }

    pub fn get_all_states(&self) -> std::collections::HashMap<String, LoroValue> {
        self.0
            .lock()
            .unwrap()
            .get_all_states()
            .into_iter()
            .map(|(k, v)| (k, v.into()))
            .collect()
    }

    pub fn subscribe_local_update(
        &self,
        listener: Arc<dyn LocalEphemeralListener>,
    ) -> Arc<Subscription> {
        let s = self
            .0
            .lock()
            .unwrap()
            .subscribe_local_updates(Box::new(move |update| {
                // TODO: should it be cloned?
                listener.on_ephemeral_update(update.to_vec());
                true
            }));
        Arc::new(Subscription(Mutex::new(Some(s))))
    }

    pub fn subscribe(&self, listener: Arc<dyn EphemeralSubscriber>) -> Arc<Subscription> {
        let s = self
            .0
            .lock()
            .unwrap()
            .subscribe(Box::new(move |update| {
                listener.on_ephemeral_event(EphemeralStoreEvent {
                    by: update.by,
                    added: update.added.clone(),
                    removed: update.removed.clone(),
                    updated: update.updated.clone(),
                });
                true
            }));
        Arc::new(Subscription(Mutex::new(Some(s))))
    }
}
