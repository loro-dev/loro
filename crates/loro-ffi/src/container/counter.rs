use std::sync::Arc;

use loro::{ContainerTrait, LoroResult};

use crate::ContainerID;

#[derive(Debug, Clone)]
pub struct LoroCounter {
    pub(crate) counter: loro::LoroCounter,
}

impl LoroCounter {
    pub fn new() -> Self {
        Self {
            counter: loro::LoroCounter::new(),
        }
    }

    /// Whether the container is attached to a document
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn is_attached(&self) -> bool {
        self.counter.is_attached()
    }

    /// If a detached container is attached, this method will return its corresponding attached handler.
    pub fn get_attached(&self) -> Option<Arc<LoroCounter>> {
        self.counter
            .get_attached()
            .map(|x| Arc::new(LoroCounter { counter: x }))
    }

    /// Return container id of the Counter.
    pub fn id(&self) -> ContainerID {
        self.counter.id().into()
    }

    /// Increment the counter by the given value.
    pub fn increment(&self, value: f64) -> LoroResult<()> {
        self.counter.increment(value)
    }

    /// Decrement the counter by the given value.
    pub fn decrement(&self, value: f64) -> LoroResult<()> {
        self.counter.decrement(value)
    }

    /// Get the current value of the counter.
    pub fn get_value(&self) -> f64 {
        self.counter.get_value()
    }

    pub fn is_deleted(&self) -> bool {
        self.counter.is_deleted()
    }
}

impl Default for LoroCounter {
    fn default() -> Self {
        Self::new()
    }
}
