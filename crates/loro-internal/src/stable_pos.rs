use loro_common::{ContainerID, ID};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StablePosition {
    // It's option because it's possible that the given container is empty.
    pub id: Option<ID>,
    pub container: ContainerID,
}

#[derive(Debug)]
pub struct PosQueryResult {
    pub update: Option<StablePosition>,
    pub current_pos: usize,
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
    pub fn new(id: Option<ID>, container: ContainerID) -> Self {
        Self { id, container }
    }
}
