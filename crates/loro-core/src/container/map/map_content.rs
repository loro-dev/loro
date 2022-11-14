use rle::{HasLength, Mergable, Sliceable};

use crate::{ContentType, InsertContentTrait, InternalString, LoroValue};

// TODO: use imported and exported format to save the space
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MapSet {
    pub(crate) key: InternalString,
    pub(crate) value: LoroValue,
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
