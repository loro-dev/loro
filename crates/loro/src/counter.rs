use loro_internal::{
    container::ContainerID, handler::counter::CounterHandler, HandlerTrait, LoroResult,
};

use crate::{Container, ContainerTrait, SealedTrait};

/// A counter that can be incremented or decremented.
#[derive(Debug, Clone)]
pub struct LoroCounter {
    pub(crate) handler: CounterHandler,
}

impl Default for LoroCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl LoroCounter {
    /// Create a new Counter.
    pub fn new() -> Self {
        Self {
            handler: CounterHandler::new_detached(),
        }
    }

    /// Return container id of the Counter.
    pub fn id(&self) -> ContainerID {
        self.handler.id().clone()
    }

    /// Increment the counter by the given value.
    pub fn increment(&self, value: f64) -> LoroResult<()> {
        self.handler.increment(value)
    }

    /// Decrement the counter by the given value.
    pub fn decrement(&self, value: f64) -> LoroResult<()> {
        self.handler.decrement(value)
    }

    /// Get the current value of the counter.
    pub fn get_value(&self) -> f64 {
        self.handler.get_value().into_double().unwrap()
    }

    /// Get the current value of the counter
    pub fn get(&self) -> f64 {
        self.handler.get_value().into_double().unwrap()
    }
}

impl SealedTrait for LoroCounter {}
impl ContainerTrait for LoroCounter {
    type Handler = CounterHandler;

    fn to_container(&self) -> Container {
        Container::Counter(self.clone())
    }

    fn to_handler(&self) -> Self::Handler {
        self.handler.clone()
    }

    fn from_handler(handler: Self::Handler) -> Self {
        Self { handler }
    }

    fn is_attached(&self) -> bool {
        self.handler.is_attached()
    }

    fn get_attached(&self) -> Option<Self> {
        self.handler.get_attached().map(Self::from_handler)
    }

    fn try_from_container(container: Container) -> Option<Self> {
        container.into_counter().ok()
    }

    fn is_deleted(&self) -> bool {
        self.handler.is_deleted()
    }
}
