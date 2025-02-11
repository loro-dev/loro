mod counter;
mod list;
mod map;
mod movable_list;
mod text;
mod tree;
mod unknown;

use std::sync::Arc;

pub use counter::LoroCounter;
pub use list::{Cursor, LoroList};
pub use map::LoroMap;
pub use movable_list::LoroMovableList;
pub use text::LoroText;
pub use tree::{LoroTree, TreeParentId};
pub use unknown::LoroUnknown;

use crate::{ContainerID, ContainerType};

#[derive(Debug, Clone)]
pub enum Container {
    /// [LoroList container](https://loro.dev/docs/tutorial/list)
    List { container: Arc<LoroList> },
    /// [LoroMap container](https://loro.dev/docs/tutorial/map)
    Map { container: Arc<LoroMap> },
    /// [LoroText container](https://loro.dev/docs/tutorial/text)
    Text { container: Arc<LoroText> },
    /// [LoroTree container]
    Tree { container: Arc<LoroTree> },
    /// [LoroMovableList container](https://loro.dev/docs/tutorial/list)
    MovableList { container: Arc<LoroMovableList> },
    /// [LoroCounter container]
    Counter { container: Arc<LoroCounter> },
    /// Unknown container
    Unknown { container: Arc<LoroUnknown> },
}

impl From<Container> for loro::Container {
    fn from(value: Container) -> Self {
        match value {
            Container::List { container } => loro::Container::List(container.inner.clone()),
            Container::Map { container } => loro::Container::Map(container.inner.clone()),
            Container::Text { container } => loro::Container::Text(container.inner.clone()),
            Container::Tree { container } => loro::Container::Tree(container.inner.clone()),
            Container::MovableList { container } => {
                loro::Container::MovableList(container.inner.clone())
            }
            Container::Counter { container } => loro::Container::Counter(container.inner.clone()),
            Container::Unknown { container } => loro::Container::Unknown(container.inner.clone()),
        }
    }
}

impl From<loro::Container> for Container {
    fn from(value: loro::Container) -> Self {
        match value {
            loro::Container::List(l) => Container::List {
                container: Arc::new(LoroList { inner: l }),
            },
            loro::Container::Map(m) => Container::Map {
                container: Arc::new(LoroMap { inner: m }),
            },
            loro::Container::Text(t) => Container::Text {
                container: Arc::new(LoroText { inner: t }),
            },
            loro::Container::Tree(t) => Container::Tree {
                container: Arc::new(LoroTree { inner: t }),
            },
            loro::Container::MovableList(l) => Container::MovableList {
                container: Arc::new(LoroMovableList { inner: l }),
            },
            loro::Container::Counter(c) => Container::Counter {
                container: Arc::new(LoroCounter { inner: c }),
            },
            loro::Container::Unknown(u) => Container::Unknown {
                container: Arc::new(LoroUnknown { inner: u }),
            },
        }
    }
}

pub trait ContainerIdLike: Send + Sync {
    fn as_container_id(&self, ty: ContainerType) -> ContainerID;
}

impl ContainerIdLike for ContainerID {
    fn as_container_id(&self, _ty: ContainerType) -> ContainerID {
        self.clone()
    }
}

impl ContainerIdLike for String {
    fn as_container_id(&self, ty: ContainerType) -> ContainerID {
        ContainerID::Root {
            name: String::from(self),
            container_type: ty,
        }
    }
}
