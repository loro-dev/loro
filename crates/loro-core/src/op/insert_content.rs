use std::any::{Any, TypeId};

use crate::id::ID;
use rle::{HasLength, Mergable, Sliceable};
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum ContentTypeID {
    Text,
    Custom(u16),
}

pub trait MergeableInsertContent {
    fn is_mergable_content(&self, other: &dyn InsertContent) -> bool;
    fn merge_content(&mut self, other: &dyn InsertContent);
}

pub trait InsertContent: HasLength + std::fmt::Debug + Any + MergeableInsertContent {
    fn id(&self) -> ContentTypeID;
    fn slice(&self, from: usize, to: usize) -> Box<dyn InsertContent>;
    fn clone_content(&self) -> Box<dyn InsertContent>;
}

impl<T: Mergable + Any> MergeableInsertContent for T {
    fn is_mergable_content(&self, other: &dyn InsertContent) -> bool {
        if self.type_id() == other.type_id() {
            self.is_mergable(unsafe { &*(other as *const dyn Any as *const T) }, &())
        } else {
            false
        }
    }

    fn merge_content(&mut self, other: &dyn InsertContent) {
        let other = unsafe { &*(other as *const dyn Any as *const T) };
        self.merge(other, &());
    }
}

pub mod content {
    use super::*;
    pub fn downcast_ref<T: Any>(content: &dyn InsertContent) -> Option<&T> {
        let t = TypeId::of::<T>();
        let concrete = content.type_id();
        if t == concrete {
            Some(unsafe { &*(content as *const dyn Any as *const T) })
        } else {
            None
        }
    }
}
