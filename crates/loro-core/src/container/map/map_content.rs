use rle::{HasLength, Mergable, Sliceable};

use crate::{id::ID, value::InsertValue, ContentType, InsertContent, InternalString};

#[derive(Clone, Debug, PartialEq)]
pub struct MapInsertContent {
    pub(crate) key: InternalString,
    pub(crate) value: InsertValue,
}

impl Mergable for MapInsertContent {}
impl Sliceable for MapInsertContent {
    fn slice(&self, from: usize, to: usize) -> Self {
        assert!(from == 0 && to == 1);
        self.clone()
    }
}
impl HasLength for MapInsertContent {
    fn len(&self) -> usize {
        1
    }
}

impl InsertContent for MapInsertContent {
    fn id(&self) -> ContentType {
        ContentType::Map
    }
}
