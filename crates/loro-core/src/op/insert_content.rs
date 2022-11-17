use std::any::{Any, TypeId};

use enum_as_inner::EnumAsInner;
use rle::{HasLength, Mergable, Sliceable};

use crate::container::{list::list_op::ListOp, map::MapSet, ContainerID};

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum ContentType {
    /// See [`crate::container::ContainerContent`]
    Container,
    /// See [`crate::container::text::TextContent`]
    Text,
    /// See [`crate::container::map::MapInsertContent`]
    Map,
    /// Users can define their own content types.
    Custom(u16),
}

#[derive(EnumAsInner, Debug)]
pub enum Content {
    Container(ContainerID),
    Map(MapSet),
    List(ListOp),
    Dyn(Box<dyn InsertContentTrait>),
}

impl Clone for Content {
    fn clone(&self) -> Self {
        match self {
            Self::Map(arg0) => Self::Map(arg0.clone()),
            Self::List(arg0) => Self::List(arg0.clone()),
            Self::Dyn(arg0) => Self::Dyn(arg0.clone_content()),
            Content::Container(arg0) => Self::Container(arg0.clone()),
        }
    }
}

impl Content {
    pub fn id(&self) -> ContentType {
        match self {
            Self::Map(_) => ContentType::Map,
            Self::List(_) => ContentType::Text,
            Self::Dyn(arg0) => arg0.id(),
            Self::Container(_) => ContentType::Container,
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

impl HasLength for Content {
    fn content_len(&self) -> usize {
        match self {
            Content::Map(x) => x.content_len(),
            Content::Dyn(x) => x.content_len(),
            Content::List(x) => x.content_len(),
            Content::Container(_) => 1,
        }
    }
}

impl Sliceable for Content {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            Content::Map(x) => Content::Map(x.slice(from, to)),
            Content::Dyn(x) => Content::Dyn(x.slice_content(from, to)),
            Content::List(x) => Content::List(x.slice(from, to)),
            Content::Container(x) => Content::Container(x.clone()),
        }
    }
}

impl Mergable for Content {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        match (self, other) {
            (Content::Map(x), Content::Map(y)) => x.is_mergable(y, &()),
            (Content::List(x), Content::List(y)) => x.is_mergable(y, &()),
            (Content::Dyn(x), Content::Dyn(y)) => x.is_mergable_content(&**y),
            (Content::Container(_), _) => false,
            _ => false,
        }
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
        match self {
            Content::Map(x) => match _other {
                Content::Map(y) => x.merge(y, &()),
                _ => unreachable!(),
            },
            Content::List(x) => match _other {
                Content::List(y) => x.merge(y, &()),
                _ => unreachable!(),
            },
            Content::Dyn(x) => x.merge_content(&**_other.as_dyn().unwrap()),
            Content::Container(_) => unreachable!(),
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
