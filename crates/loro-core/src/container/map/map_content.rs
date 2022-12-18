use rle::{HasLength, Mergable, Sliceable};
use serde::{Deserialize, Serialize};

use crate::{ContentType, InsertContentTrait, InternalString, LoroValue};

// Note: It will be encoded into binary format, so the order of its fields should not be changed.
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

#[cfg(test)]
mod test {
    use super::MapSet;
    #[test]
    fn fix_fields_order() {
        let map_set = vec![MapSet {
            key: "key".to_string().into(),
            value: "value".to_string().into(),
        }];
        let map_set_buf = vec![1, 3, 107, 101, 121, 4, 5, 118, 97, 108, 117, 101];
        assert_eq!(
            postcard::from_bytes::<Vec<MapSet>>(&map_set_buf).unwrap(),
            map_set
        );
    }
}
