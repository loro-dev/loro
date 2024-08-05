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

// pub trait ContainerLike: Sync + Send {
//     fn to_container(&self) -> loro::Container;
// }

// impl<T: ContainerLike> ContainerLike for Arc<T> {
//     fn to_container(&self) -> loro::Container {
//         self.as_ref().to_container()
//     }
// }

// impl ContainerLike for LoroList {
//     fn to_container(&self) -> loro::Container {
//         loro::Container::List(self.list.clone())
//     }
// }
// impl ContainerLike for LoroMap {
//     fn to_container(&self) -> loro::Container {
//         loro::Container::Map(self.map.clone())
//     }
// }
// impl ContainerLike for LoroMovableList {
//     fn to_container(&self) -> loro::Container {
//         loro::Container::MovableList(self.list.clone())
//     }
// }
// impl ContainerLike for LoroText {
//     fn to_container(&self) -> loro::Container {
//         loro::Container::Text(self.text.clone())
//     }
// }
// impl ContainerLike for LoroTree {
//     fn to_container(&self) -> loro::Container {
//         loro::Container::Tree(self.tree.clone())
//     }
// }
// impl ContainerLike for LoroCounter {
//     fn to_container(&self) -> loro::Container {
//         loro::Container::Counter(self.counter.clone())
//     }
// }
// impl ContainerLike for LoroUnknown {
//     fn to_container(&self) -> loro::Container {
//         loro::Container::Unknown(self.unknown.clone())
//     }
// }
