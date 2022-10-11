use std::{
    borrow::BorrowMut,
    cell::{Ref, RefCell, RefMut},
    ops::Range,
    rc::Rc,
};

use rle::{rle_tree::tree_trait::*, HasLength, Mergable, Sliceable};
use smartstring::SmartString;

type SString = SmartString<smartstring::LazyCompact>;

#[derive(Debug, Clone)]
pub struct CustomString {
    str: Rc<RefCell<SString>>,
    slice: Range<usize>,
}

impl CustomString {
    fn str(&self) -> Ref<'_, SString> {
        RefCell::borrow(&self.str)
    }

    fn str_mut(&self) -> RefMut<'_, SString> {
        RefCell::borrow_mut(&self.str)
    }
}

impl HasLength for CustomString {
    fn len(&self) -> usize {
        rle::HasLength::len(&self.slice)
    }
}

impl Mergable for CustomString {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        self.slice.start == 0
            && self.slice.end == self.str().len()
            && self.str().capacity() > other.len() + self.str().len()
            && Rc::strong_count(&self.str) == 1
    }

    fn merge(&mut self, other: &Self, _conf: &())
    where
        Self: Sized,
    {
        self.str_mut().push_str(other.str().as_str());
        let length = self.str().len();
        self.borrow_mut().slice.end = length;
    }
}

impl Sliceable for CustomString {
    fn slice(&self, from: usize, to: usize) -> Self {
        CustomString {
            str: self.str.clone(),
            slice: self.slice.start + from..self.slice.start + to,
        }
    }
}

pub type StringTreeTrait = CumulateTreeTrait<CustomString, 4>;

impl From<&str> for CustomString {
    fn from(origin: &str) -> Self {
        CustomString {
            str: Rc::new(RefCell::new(SString::from(origin))),
            slice: 0..origin.len(),
        }
    }
}
