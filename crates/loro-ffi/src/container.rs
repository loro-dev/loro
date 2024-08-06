mod counter;
mod list;
mod map;
mod movable_list;
mod text;
mod tree;
mod unknown;

pub use counter::LoroCounter;
pub use list::{Cursor, LoroList};
pub use map::LoroMap;
pub use movable_list::LoroMovableList;
pub use text::LoroText;
pub use tree::LoroTree;
pub use unknown::LoroUnknown;

use crate::{ContainerID, ContainerType};

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
