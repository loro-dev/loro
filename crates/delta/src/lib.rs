use delta_rope::rle_tree::DeltaTreeTrait;
use delta_trait::{DeltaAttr, DeltaValue};
use enum_as_inner::EnumAsInner;
use generic_btree::{
    rle::{HasLength, Mergeable, Sliceable},
    BTree,
};
use std::fmt::Debug;

pub mod array_vec;
mod delta_item;
mod delta_rope;
pub mod delta_trait;
pub mod iter;
pub mod text_delta;
pub mod utf16;

/// A [DeltaRope] is a rope-like data structure that can be used to represent
/// a sequence of [DeltaItem]. It has efficient operations for composing other
/// [DeltaRope]s. It can also be used as a rope, where it only contains insertions.
#[derive(Debug, Clone)]
pub struct DeltaRope<V: DeltaValue, Attr: DeltaAttr> {
    tree: BTree<DeltaTreeTrait<V, Attr>>,
}

pub struct DeltaRopeBuilder<V: DeltaValue, Attr: DeltaAttr> {
    items: Vec<DeltaItem<V, Attr>>,
}

#[derive(Debug, Clone, PartialEq, Eq, EnumAsInner)]
pub enum DeltaItem<V, Attr> {
    Retain { len: usize, attr: Attr },
    Insert { value: V, attr: Attr },
    Delete(usize),
}
