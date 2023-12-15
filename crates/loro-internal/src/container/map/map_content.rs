use rle::{HasLength, Mergable, Sliceable};
use serde::{Deserialize, Serialize};

use crate::{InternalString, LoroValue};

// Note: It will be encoded into binary format, so the order of its fields should not be changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapSet {
    pub(crate) key: InternalString,
    // the key is deleted if value is None
    pub(crate) value: Option<LoroValue>,
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

#[cfg(test)]
mod test {
    use super::MapSet;
    #[test]
    fn fix_fields_order() {
        let map_set = vec![MapSet {
            key: "key".to_string().into(),
            value: Some("value".to_string().into()),
        }];
        let map_set_buf = vec![1, 3, 107, 101, 121, 1, 4, 5, 118, 97, 108, 117, 101];
        assert_eq!(
            postcard::from_bytes::<Vec<MapSet>>(&map_set_buf).unwrap(),
            map_set
        );
    }
}
