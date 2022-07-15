use crate::{InsertContent, SmString, ID};
use rle::{HasLength, Mergable, Sliceable};
use std::alloc::Layout;

mod container_content;
pub use container_content::*;

pub trait Container {}
