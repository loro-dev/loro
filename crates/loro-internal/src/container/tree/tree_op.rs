use loro_common::TreeID;
use rle::{HasLength, Mergable, Sliceable};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct TreeOp {
    pub(crate) target: TreeID,
    pub(crate) parent: Option<TreeID>,
}

impl HasLength for TreeOp {
    fn content_len(&self) -> usize {
        1
    }
}

impl Sliceable for TreeOp {
    fn slice(&self, from: usize, to: usize) -> Self {
        assert!(from == 0 && to == 1);
        *self
    }
}

impl Mergable for TreeOp {}
