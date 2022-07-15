use std::any::{Any, TypeId};

use crate::id::ID;
use rle::{HasLength, Mergable, Sliceable};

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum ContentType {
    /// See [`crate::container::ContainerContent`]
    Container,
    /// See [`crate::text::TextContent`]
    Text,
    /// Users can define their own content types.
    Custom(u16),
}

pub trait MergeableContent {
    fn is_mergable_content(&self, other: &dyn InsertContent) -> bool;
    fn merge_content(&mut self, other: &dyn InsertContent);
}

pub trait SliceableContent {
    fn slice_content(&self, from: usize, to: usize) -> Box<dyn InsertContent>;
}

pub trait CloneContent {
    fn clone_content(&self) -> Box<dyn InsertContent>;
}

pub trait InsertContent:
    HasLength + std::fmt::Debug + Any + MergeableContent + SliceableContent + CloneContent
{
    fn id(&self) -> ContentType;
}

impl<T: Sliceable + InsertContent> SliceableContent for T {
    fn slice_content(&self, from: usize, to: usize) -> Box<dyn InsertContent> {
        Box::new(self.slice(from, to))
    }
}

impl<T: Clone + InsertContent> CloneContent for T {
    fn clone_content(&self) -> Box<dyn InsertContent> {
        Box::new(self.clone())
    }
}

impl<T: Mergable + Any> MergeableContent for T {
    fn is_mergable_content(&self, other: &dyn InsertContent) -> bool {
        if self.type_id() == other.type_id() {
            self.is_mergable(
                unsafe { &*(other as *const dyn InsertContent as *const T) },
                &(),
            )
        } else {
            false
        }
    }

    fn merge_content(&mut self, other: &dyn InsertContent) {
        let other = unsafe { &*(other as *const dyn InsertContent as *const T) };
        self.merge(other, &());
    }
}

pub mod content {
    use super::*;
    pub fn downcast_ref<T: Any>(content: &dyn InsertContent) -> Option<&T> {
        let t = TypeId::of::<T>();
        let concrete = content.type_id();
        if t == concrete {
            Some(unsafe { &*(content as *const dyn InsertContent as *const T) })
        } else {
            None
        }
    }
}
