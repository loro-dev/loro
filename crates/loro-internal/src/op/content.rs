use std::any::Any;

use enum_as_inner::EnumAsInner;
use rle::{HasLength, Mergable, Sliceable};
use serde::{Deserialize, Serialize};

use crate::container::{
    list::list_op::{InnerListOp, ListOp},
    map::{InnerMapSet, MapSet},
};

/// @deprecated
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum ContentType {
    /// See [`crate::container::text::TextContent`]
    List,
    /// See [`crate::container::map::MapInsertContent`]
    Map,
    /// Users can define their own content types.
    Custom(u16),
}

#[derive(EnumAsInner, Debug, Clone)]
pub enum InnerContent {
    List(InnerListOp),
    Map(InnerMapSet),
}

// Note: It will be encoded into binary format, so the order of its fields should not be changed.
#[derive(EnumAsInner, Debug, PartialEq, Serialize, Deserialize)]
pub enum RawOpContent<'a> {
    Map(MapSet),
    List(ListOp<'a>),
}

impl<'a> Clone for RawOpContent<'a> {
    fn clone(&self) -> Self {
        match self {
            Self::Map(arg0) => Self::Map(arg0.clone()),
            Self::List(arg0) => Self::List(arg0.clone()),
        }
    }
}

impl<'a> RawOpContent<'a> {
    pub fn to_static(&self) -> RawOpContent<'static> {
        match self {
            Self::Map(arg0) => RawOpContent::Map(arg0.clone()),
            Self::List(arg0) => match arg0 {
                ListOp::Insert { slice, pos } => RawOpContent::List(ListOp::Insert {
                    slice: slice.to_static(),
                    pos: *pos,
                }),
                ListOp::Delete(x) => RawOpContent::List(ListOp::Delete(*x)),
            },
        }
    }
}

/// @deprecated
pub trait MergeableContent {
    fn is_mergable_content(&self, other: &dyn InsertContentTrait) -> bool;
    fn merge_content(&mut self, other: &dyn InsertContentTrait);
}

/// @deprecated
pub trait SliceableContent {
    fn slice_content(&self, from: usize, to: usize) -> Box<dyn InsertContentTrait>;
}

/// @deprecated
pub trait CloneContent {
    fn clone_content(&self) -> Box<dyn InsertContentTrait>;
}

/// @deprecated
pub trait InsertContentTrait:
    HasLength + std::fmt::Debug + Any + MergeableContent + SliceableContent + CloneContent
{
    fn id(&self) -> ContentType;
    // TODO: provide an encoding method
}

impl<T: Sliceable + InsertContentTrait> SliceableContent for T {
    fn slice_content(&self, from: usize, to: usize) -> Box<dyn InsertContentTrait> {
        Box::new(self.slice(from, to))
    }
}

impl<T: Clone + InsertContentTrait> CloneContent for T {
    fn clone_content(&self) -> Box<dyn InsertContentTrait> {
        Box::new(self.clone())
    }
}

impl<'a> HasLength for RawOpContent<'a> {
    fn content_len(&self) -> usize {
        match self {
            RawOpContent::Map(x) => x.content_len(),
            RawOpContent::List(x) => x.content_len(),
        }
    }
}

impl<'a> Sliceable for RawOpContent<'a> {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            RawOpContent::Map(x) => RawOpContent::Map(x.slice(from, to)),
            RawOpContent::List(x) => RawOpContent::List(x.slice(from, to)),
        }
    }
}

impl<'a> Mergable for RawOpContent<'a> {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        match (self, other) {
            (RawOpContent::Map(x), RawOpContent::Map(y)) => x.is_mergable(y, &()),
            (RawOpContent::List(x), RawOpContent::List(y)) => x.is_mergable(y, &()),
            _ => false,
        }
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
        match self {
            RawOpContent::Map(x) => match _other {
                RawOpContent::Map(y) => x.merge(y, &()),
                _ => unreachable!(),
            },
            RawOpContent::List(x) => match _other {
                RawOpContent::List(y) => x.merge(y, &()),
                _ => unreachable!(),
            },
        }
    }
}

impl HasLength for InnerContent {
    fn content_len(&self) -> usize {
        match self {
            InnerContent::List(list) => list.atom_len(),
            InnerContent::Map(_) => 1,
        }
    }
}

impl Sliceable for InnerContent {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            a @ InnerContent::Map(_) => a.clone(),
            InnerContent::List(x) => InnerContent::List(x.slice(from, to)),
        }
    }
}

impl Mergable for InnerContent {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        match (self, other) {
            (InnerContent::List(x), InnerContent::List(y)) => x.is_mergable(y, &()),
            _ => false,
        }
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
        match self {
            InnerContent::List(x) => match _other {
                InnerContent::List(y) => x.merge(y, &()),
                _ => unreachable!(),
            },
            InnerContent::Map(_) => unreachable!(),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::container::{
        list::list_op::{DeleteSpan, ListOp},
        map::MapSet,
    };

    use super::RawOpContent;

    #[test]
    fn fix_fields_order() {
        let remote_content = vec![
            RawOpContent::List(ListOp::Delete(DeleteSpan { pos: 0, len: 1 })),
            RawOpContent::Map(MapSet {
                key: "a".to_string().into(),
                value: Some("b".to_string().into()),
            }),
        ];
        let remote_content_buf = vec![2, 1, 1, 0, 2, 0, 1, 97, 1, 4, 1, 98];
        assert_eq!(
            postcard::from_bytes::<Vec<RawOpContent>>(&remote_content_buf).unwrap(),
            remote_content
        );
    }
}
