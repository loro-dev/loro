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
