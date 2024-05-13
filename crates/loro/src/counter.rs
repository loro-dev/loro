use loro_internal::{
    container::ContainerID, handler::counter::CounterHandler, HandlerTrait, LoroResult, LoroValue,
};

use crate::{Container, ContainerTrait, SealedTrait};

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
    pub fn new() -> Self {
        Self {
            handler: CounterHandler::new_detached(),
        }
    }

    /// Return container id of the Counter.
    pub fn id(&self) -> ContainerID {
        self.handler.id().clone()
    }

    pub fn increment(&self, value: i64) -> LoroResult<()> {
        self.handler.increment(value)
    }

    pub fn get_value(&self) -> LoroValue {
        self.handler.get_value()
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
}
