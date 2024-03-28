use std::fmt::Display;

use loro_common::{ContainerID, IdLp};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RelativePosition {
    pub id: IdLp,
    pub container: ContainerID,
}

#[derive(Debug)]
pub struct PosQueryResult {
    pub updated_pos: RelativePosition,
    pub pos: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum CannotFindRelativePosition {
    ContainerDeleted,
    HistoryCleared,
}
impl Display for CannotFindRelativePosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CannotFindRelativePosition::ContainerDeleted => {
                f.write_str("Cannot find relative position. The container is deleted.")
            }
            CannotFindRelativePosition::HistoryCleared => {
                f.write_str("Cannot find relative position. It may be that the given id is deleted and the relative history is cleared.")
            },
        }
    }
}

impl RelativePosition {
    pub fn new(id: IdLp, container: ContainerID) -> Self {
        Self { id, container }
    }
}
