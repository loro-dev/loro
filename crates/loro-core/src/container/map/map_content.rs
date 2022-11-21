use rle::{HasLength, Mergable, Sliceable};

use crate::{ContentType, InsertContentTrait, InternalString, LoroValue};

#[derive(Clone, Debug, PartialEq)]
pub struct MapSet {
    pub(crate) key: InternalString,
    pub(crate) value: LoroValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InnerMapSet {
    pub(crate) key: InternalString,
    pub(crate) value: usize,
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

impl InsertContentTrait for MapSet {
    fn id(&self) -> ContentType {
        ContentType::Map
    }
}
