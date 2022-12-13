use std::any::{Any, TypeId};

use enum_as_inner::EnumAsInner;
use rle::{HasLength, Mergable, Sliceable};
use serde::{Deserialize, Serialize};

use crate::container::{
    list::list_op::{InnerListOp, ListOp},
    map::{InnerMapSet, MapSet},
};

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
pub enum RemoteContent {
    Map(MapSet),
    List(ListOp),
}

impl Clone for RemoteContent {
    fn clone(&self) -> Self {
        match self {
            Self::Map(arg0) => Self::Map(arg0.clone()),
            Self::List(arg0) => Self::List(arg0.clone()),
        }
    }
}

pub trait MergeableContent {
    fn is_mergable_content(&self, other: &dyn InsertContentTrait) -> bool;
    fn merge_content(&mut self, other: &dyn InsertContentTrait);
}

pub trait SliceableContent {
    fn slice_content(&self, from: usize, to: usize) -> Box<dyn InsertContentTrait>;
}

pub trait CloneContent {
    fn clone_content(&self) -> Box<dyn InsertContentTrait>;
}

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

impl<T: Mergable + Any> MergeableContent for T {
    fn is_mergable_content(&self, other: &dyn InsertContentTrait) -> bool {
        if self.type_id() == other.type_id() {
            let other: &T = utils::downcast_ref(other).unwrap();
            self.is_mergable(other, &())
        } else {
            false
        }
    }

    fn merge_content(&mut self, other: &dyn InsertContentTrait) {
        let other: &T = utils::downcast_ref(other).unwrap();
        self.merge(other, &());
    }
}

impl HasLength for RemoteContent {
    fn content_len(&self) -> usize {
        match self {
            RemoteContent::Map(x) => x.content_len(),
            RemoteContent::List(x) => x.content_len(),
        }
    }
}

impl Sliceable for RemoteContent {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            RemoteContent::Map(x) => RemoteContent::Map(x.slice(from, to)),
            RemoteContent::List(x) => RemoteContent::List(x.slice(from, to)),
        }
    }
}

impl Mergable for RemoteContent {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        match (self, other) {
            (RemoteContent::Map(x), RemoteContent::Map(y)) => x.is_mergable(y, &()),
            (RemoteContent::List(x), RemoteContent::List(y)) => x.is_mergable(y, &()),
            _ => false,
        }
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
        match self {
            RemoteContent::Map(x) => match _other {
                RemoteContent::Map(y) => x.merge(y, &()),
                _ => unreachable!(),
            },
            RemoteContent::List(x) => match _other {
                RemoteContent::List(y) => x.merge(y, &()),
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

pub mod utils {
    use super::*;
    pub fn downcast_ref<T: Any>(content: &dyn InsertContentTrait) -> Option<&T> {
        let t = TypeId::of::<T>();
        let concrete = content.type_id();
        if t == concrete {
            // SAFETY: we know that the type is correct
            Some(unsafe { &*(content as *const dyn InsertContentTrait as *const T) })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod test {
    use crate::container::{
        list::list_op::{DeleteSpan, ListOp},
        map::MapSet,
    };

    use super::RemoteContent;

    #[test]
    fn fix_fields_order() {
        let remote_content = vec![
            RemoteContent::List(ListOp::Delete(DeleteSpan { pos: 0, len: 1 })),
            RemoteContent::Map(MapSet {
                key: "a".to_string().into(),
                value: "b".to_string().into(),
            }),
        ];
        let remote_content_buf = vec![2, 1, 1, 0, 2, 0, 1, 97, 4, 1, 98];
        assert_eq!(
            postcard::from_bytes::<Vec<RemoteContent>>(&remote_content_buf).unwrap(),
            remote_content
        );
    }
}
