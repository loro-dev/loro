use std::any::{Any, TypeId};

use enum_as_inner::EnumAsInner;
use rle::{HasLength, Mergable, Sliceable};

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

#[derive(EnumAsInner, Debug)]
pub enum InnerContent {
    Unknown(usize),
    List(InnerListOp),
    Map(InnerMapSet),
}

#[derive(EnumAsInner, Debug)]
pub enum RemoteContent {
    Map(MapSet),
    List(ListOp),
    Dyn(Box<dyn InsertContentTrait>),
}

impl Clone for RemoteContent {
    fn clone(&self) -> Self {
        match self {
            Self::Map(arg0) => Self::Map(arg0.clone()),
            Self::List(arg0) => Self::List(arg0.clone()),
            Self::Dyn(arg0) => Self::Dyn(arg0.clone_content()),
        }
    }
}

impl RemoteContent {
    pub fn id(&self) -> ContentType {
        match self {
            Self::Map(_) => ContentType::Map,
            Self::List(_) => ContentType::List,
            Self::Dyn(arg0) => arg0.id(),
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
            RemoteContent::Dyn(x) => x.content_len(),
            RemoteContent::List(x) => x.content_len(),
        }
    }
}

impl Sliceable for RemoteContent {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            RemoteContent::Map(x) => RemoteContent::Map(x.slice(from, to)),
            RemoteContent::Dyn(x) => RemoteContent::Dyn(x.slice_content(from, to)),
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
            (RemoteContent::Dyn(x), RemoteContent::Dyn(y)) => x.is_mergable_content(&**y),
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
            RemoteContent::Dyn(x) => x.merge_content(&**_other.as_dyn().unwrap()),
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
