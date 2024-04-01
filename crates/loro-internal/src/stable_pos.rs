use loro_common::{ContainerID, ID};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StablePosition {
    // It's option because it's possible that the given container is empty.
    pub id: Option<ID>,
    pub container: ContainerID,
    /// The target position is at the left, middle, or right of the given id.
    ///
    /// Side info can help to model the selection
    pub side: Side,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Left = -1,
    Middle = 0,
    Right = 1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosQueryResult {
    pub update: Option<StablePosition>,
    pub current: Cursor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub pos: usize,
    /// The target position is at the left, middle, or right of the given pos.
    pub side: Side,
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum CannotFindRelativePosition {
    #[error("Cannot find relative position. The container is deleted.")]
    ContainerDeleted,
    #[error("Cannot find relative position. It may be that the given id is deleted and the relative history is cleared.")]
    HistoryCleared,
    #[error("Cannot find relative position. The id is not found.")]
    IdNotFound,
}

impl StablePosition {
    pub fn new(id: Option<ID>, container: ContainerID, side: Side) -> Self {
        Self {
            id,
            container,
            side,
        }
    }
}
