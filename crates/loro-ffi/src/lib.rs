mod value;
pub use value::{ContainerID, ContainerType, LoroValue};
mod doc;
pub use doc::LoroDoc;
mod container;
pub use container::Container;
mod event;
mod list;

pub enum ValueOrContainer {
    Value { value: LoroValue },
    Container { container: Container },
}
