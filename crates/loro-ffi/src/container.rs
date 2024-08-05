mod counter;
mod list;
mod map;
mod movable_list;
mod text;
mod tree;
mod unknown;
pub use counter::LoroCounter;
pub use list::LoroList;
pub use map::LoroMap;
pub use movable_list::LoroMovableList;
pub use text::LoroText;
pub use tree::LoroTree;
pub use unknown::LoroUnknown;

pub enum Container {
    /// [LoroList container](https://loro.dev/docs/tutorial/list)
    List(LoroList),
    /// [LoroMap container](https://loro.dev/docs/tutorial/map)
    Map(LoroMap),
    /// [LoroText container](https://loro.dev/docs/tutorial/text)
    Text(LoroText),
    /// [LoroTree container]
    Tree(LoroTree),
    /// [LoroMovableList container](https://loro.dev/docs/tutorial/list)
    MovableList(LoroMovableList),
    /// [LoroCounter container]
    Counter(LoroCounter),
    /// Unknown container
    Unknown(LoroUnknown),
}
