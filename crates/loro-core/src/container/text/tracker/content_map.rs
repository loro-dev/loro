use rle::RleTree;

use super::y_span::{YSpan, YSpanTreeTrait};

pub(super) type ContentMap = RleTree<YSpan, YSpanTreeTrait>;
