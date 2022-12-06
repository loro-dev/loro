use rle::{HasLength, Mergable, Sliceable};
use serde::{Deserialize, Serialize};

use crate::{ContentType, InsertContentTrait, InternalString, LoroValue};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapSet {
    pub(crate) key: InternalString,
    pub(crate) value: LoroValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InnerMapSet {
    pub(crate) key: InternalString,
    pub(crate) value: u32,
}

impl Mergable for MapSet {}
impl Sliceable for MapSet {
    fn slice(&self, from: usize, to: usize) -> Self {
        assert!(from == 0 && to == 1);
        self.clone()
    }
}
impl HasLength for MapSet {
    fn content_len(&self) -> usize {
        1
    }
}

impl Mergable for InnerMapSet {}
impl Sliceable for InnerMapSet {
    fn slice(&self, from: usize, to: usize) -> Self {
        assert!(from == 0 && to == 1);
        self.clone()
    }
}
impl HasLength for InnerMapSet {
    fn content_len(&self) -> usize {
        1
    }
}

impl InsertContentTrait for MapSet {
    fn id(&self) -> ContentType {
        ContentType::Map
    }
}
