use delta_trait::{DeltaAttr, DeltaValue};
use generic_btree::{
    rle::{HasLength, Mergeable, Sliceable},
    BTree,
};
use std::fmt::Debug;

mod delta_item;
mod delta_rope;
pub mod delta_trait;
pub mod text_delta;
pub mod utf16;

/// A [DeltaRope] is a rope-like data structure that can be used to represent
/// a sequence of [DeltaItem]. It has efficient operations for composing other
/// [DeltaRope]s. It can also be used as a rope, where it only contains insertions.
pub struct DeltaRope<V: DeltaValue, Attr: DeltaAttr> {
    tree: BTree<delta_rope::rle_tree::DeltaTreeTrait<V, Attr>>,
}

#[derive(Debug, Clone)]
pub enum DeltaItem<V, Attr> {
    Delete(usize),
    Retain { len: usize, attr: Attr },
    Insert { value: V, attr: Attr },
}
