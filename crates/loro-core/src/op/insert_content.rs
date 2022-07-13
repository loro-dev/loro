use std::any::{Any, TypeId};

use crate::id::ID;
use rle::{HasLength, Mergable, Sliceable};
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum ContentTypeID {
    Text,
    Custom(u16),
}

pub trait InsertContent: HasLength + std::fmt::Debug + Any {
    fn id(&self) -> ContentTypeID;
    fn is_mergable(&self, other: &dyn InsertContent) -> bool;
    fn merge(&mut self, other: &dyn InsertContent);
    fn slice(&self, from: usize, to: usize) -> Box<dyn InsertContent>;
    fn clone_content(&self) -> Box<dyn InsertContent>;
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
