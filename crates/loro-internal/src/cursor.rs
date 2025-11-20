use loro_common::{ContainerID, ID};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Cursor {
    // It's option because it's possible that the given container is empty.
    pub id: Option<ID>,
    pub container: ContainerID,
    /// The target position is at the left, middle, or right of the given id.
    ///
    /// Side info can help to model the selection
    pub side: Side,
    /// The position of the cursor in the container when the cursor is created.
    /// For text, this is the unicode codepoint index
    /// This value is not encoded
    pub(crate) origin_pos: usize,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Left = -1,
    #[default]
    Middle = 0,
    Right = 1,
}

impl Side {
    pub fn from_i32(i: i32) -> Option<Self> {
        match i {
            -1 => Some(Side::Left),
            0 => Some(Side::Middle),
            1 => Some(Side::Right),
            _ => None,
        }
    }

    pub fn to_i32(&self) -> i32 {
        *self as i32
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosQueryResult {
    pub update: Option<Cursor>,
    pub current: AbsolutePosition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbsolutePosition {
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

impl Cursor {
    pub fn new(id: Option<ID>, container: ContainerID, side: Side, origin_pos: usize) -> Self {
        Self {
            id,
            container,
            side,
            origin_pos,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap()
    }

    pub fn decode(data: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(data)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PosType {
    /// The index is based on the length of the text in bytes(UTF-8).
    Bytes,
    /// The index is based on the length of the text in Unicode Code Points.
    Unicode,
    /// The index is based on the length of the text in UTF-16 code units.
    Utf16,
    /// The index is based on the length of the text in events.
    /// It is determined by the `wasm` feature.
    Event,
    /// The index is based on the entity index.
    Entity,
}