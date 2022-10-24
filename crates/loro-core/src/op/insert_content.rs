use std::any::{Any, TypeId};

use enum_as_inner::EnumAsInner;
use rle::{HasLength, Mergable, Sliceable};

use crate::container::{list::list_op::ListOp, map::MapSet};

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
pub(crate) enum InsertContent {
    Map(MapSet),
    List(ListOp),
    Dyn(Box<dyn InsertContentTrait>),
}

impl Clone for InsertContent {
    fn clone(&self) -> Self {
        match self {
            Self::Map(arg0) => Self::Map(arg0.clone()),
            Self::List(arg0) => Self::List(arg0.clone()),
            Self::Dyn(arg0) => Self::Dyn(arg0.clone_content()),
        }
    }
}

impl InsertContent {
    pub fn id(&self) -> ContentType {
        match self {
            Self::Map(_) => ContentType::Map,
            Self::List(_) => ContentType::Text,
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

impl HasLength for InsertContent {
    fn content_len(&self) -> usize {
        match self {
            InsertContent::Map(x) => x.content_len(),
            InsertContent::Dyn(x) => x.content_len(),
            InsertContent::List(x) => x.content_len(),
        }
    }
}

impl Sliceable for InsertContent {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            InsertContent::Map(x) => InsertContent::Map(x.slice(from, to)),
            InsertContent::Dyn(x) => InsertContent::Dyn(x.slice_content(from, to)),
            InsertContent::List(x) => InsertContent::List(x.slice(from, to)),
        }
    }
}

impl Mergable for InsertContent {
    fn is_mergable(&self, _other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        match self {
            InsertContent::Map(x) => match _other {
                InsertContent::Map(y) => x.is_mergable(y, &()),
                _ => false,
            },
            InsertContent::List(x) => match _other {
                InsertContent::List(y) => x.is_mergable(y, &()),
                _ => false,
            },
            InsertContent::Dyn(x) => x.is_mergable_content(&**_other.as_dyn().unwrap()),
        }
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
        match self {
            InsertContent::Map(x) => match _other {
                InsertContent::Map(y) => x.merge(y, &()),
                _ => unreachable!(),
            },
            InsertContent::List(x) => match _other {
                InsertContent::List(y) => x.merge(y, &()),
                _ => unreachable!(),
            },
            InsertContent::Dyn(x) => x.merge_content(&**_other.as_dyn().unwrap()),
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
