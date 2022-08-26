use rle::{HasLength, Mergable, Sliceable};

use crate::{id::ID, value::InsertValue, ContentType, InsertContent, InternalString};

#[derive(Clone, Debug, PartialEq)]
pub struct ListInsertContent {
    pub(crate) key: u32,
    pub(crate) value: InsertValue,
}

impl Mergable for ListInsertContent {}
impl Sliceable for ListInsertContent {
    fn slice(&self, from: usize, to: usize) -> Self {
        assert!(from == 0 && to == 1);
        self.clone()
    }
}
impl HasLength for ListInsertContent {
    fn len(&self) -> usize {
        1
    }
}

impl InsertContent for ListInsertContent {
    fn id(&self) -> ContentType {
        ContentType::Map
    }
}
