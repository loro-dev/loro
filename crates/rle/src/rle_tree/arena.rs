use super::BumpVec;
use std::{
    fmt::Debug,
    ops::{Deref, DerefMut, Index, RangeBounds, IndexMut},
};


/// [BumpMode] will use [bumpalo] to allocate nodes, where allocation is fast but no deallocation happens before [crate::RleTree] dropped.
///
/// NOTE: Should be cautious when using [BumpMode] mode, T's drop method won't be called in this mode.
/// So you cannot use smart pointer in [BumpMode] mode directly. You should wrap it inside [bumpalo]'s [bumpalo::boxed::Box];
#[derive(Debug, Default)]
pub struct BumpMode(bumpalo::Bump);

fn test() {
    let _a = vec![1, 2];
}

pub trait VecTrait<'v, T>:
    Index<usize, Output = T> + IndexMut<usize> + Deref<Target = [T]> + DerefMut + Debug
{
    type Arena;
    type Drain<'a>: Iterator<Item = T>
    where
        Self:'a;

    fn drain<R>(&mut self, range: R) -> Self::Drain<'_>
    where
        R: RangeBounds<usize>;

    fn push(&mut self, value: T);
    fn pop(&mut self) -> Option<T>;
    fn clear(&mut self);
    fn insert(&mut self, index: usize, value: T);
    fn with_capacity_in(capacity: usize, arena: &'v Self::Arena) -> Self;
    fn splice<R, I>(&mut self, range: R, replace_with: I)
    where
        R: RangeBounds<usize>,
        I: IntoIterator<Item = T>;
}

pub trait Arena: Debug + Default {
    type Boxed<'a, T>: Debug + Deref<Target = T> + DerefMut
    where
        Self: 'a,
        T: 'a + Debug;

    type Vec<'v, T>: VecTrait<'v, T, Arena = Self>
    where
        Self: 'v,
        T: 'v + Debug;

    fn allocate<'a, T>(&'a self, value: T) -> Self::Boxed<'a, T>
    where
        T: 'a + Debug;

    fn allocated_bytes(&self) -> usize;
}

impl<'bump, T: Debug + 'bump> VecTrait<'bump, T> for BumpVec<'bump, T> {
    type Drain<'a> = bumpalo::collections::vec::Drain<'a, 'bump, T> 
    where 
        Self: 'a;

    #[inline(always)]
    fn drain< R>(& mut self, range: R) -> Self::Drain<'_>
    where
        R: RangeBounds<usize>,
    {
        // SAFETY: The lifetime of the returned iterator is bound to the lifetime of the arena.
        unsafe{ std::mem::transmute(self.drain(range))}
    }

    #[inline(always)]
    fn push(&mut self, value: T) {
        self.push(value)
    }

    #[inline(always)]
    fn pop(&mut self) -> Option<T> {
        self.pop()
    }

    #[inline(always)]
    fn clear(&mut self) {
        self.clear()
    }

    type Arena = BumpMode;

    #[inline(always)]
    fn insert(&mut self, index: usize, value: T) {
        self.insert(index, value)
    }

    #[inline(always)]
    fn with_capacity_in(capacity: usize, arena: &'bump Self::Arena) -> BumpVec<'bump, T> {
        BumpVec::with_capacity_in(capacity, &arena.0)
    }

    #[inline(always)]
    fn splice<R, I>(&mut self, range: R, replace_with: I)
    where
        R: RangeBounds<usize>,
        I: IntoIterator<Item = T>,
    {
        self.splice(range, replace_with);
    }
}

impl<'v, T: Debug + 'v> VecTrait<'v, T> for Vec<T> {
    type Drain<'a> = std::vec::Drain<'a, T> 
    where 
        Self: 'a,
        Self: 'v,
        T: 'a;

    #[inline(always)]
    fn drain<R>(&mut self, range: R) -> Self::Drain<'_>
    where
        R: RangeBounds<usize>,
    {
        self.drain(range)
    }

    #[inline(always)]
    fn push(&mut self, value: T) {
        self.push(value)
    }

    #[inline(always)]
    fn pop(&mut self) -> Option<T> {
        self.pop()
    }

    #[inline(always)]
    fn clear(&mut self) {
        self.clear()
    }

    type Arena = HeapMode;

    #[inline(always)]
    fn insert(&mut self, index: usize, value: T) {
        self.insert(index, value)
    }

    #[inline(always)]
    fn with_capacity_in(capacity: usize, _: &Self::Arena) -> Self {
        Vec::with_capacity(capacity)
    }

    #[inline(always)]
    fn splice<R, I>(&mut self, range: R, replace_with: I)
    where
        R: RangeBounds<usize>,
        I: IntoIterator<Item = T>,
    {
        self.splice(range, replace_with);
    }
}

impl Arena for BumpMode {
    type Boxed<'a, T> = &'a mut T where T: 'a + Debug;
    type Vec<'a, T> = BumpVec<'a, T> where T: 'a + Debug;

    fn allocate<'a, T>(&'a self, value: T) -> Self::Boxed<'a, T>
    where
        T: 'a + Debug,
    {
        self.0.alloc(value)
    }

    fn allocated_bytes(&self) -> usize {
        bumpalo::Bump::allocated_bytes(&self.0)
    }
}

/// [HeapMode] will use [Box] and [Vec] to allocate nodes for [crate::RleTree]
#[derive(Debug, Default)]
pub struct HeapMode;

impl Arena for HeapMode {
    type Boxed<'a, T> = Box<T> where T: 'a + Debug;
    type Vec<'a, T> = Vec<T> where T: 'a + Debug;

    fn allocate<'a, T>(&'a self, value: T) -> Self::Boxed<'a, T>
    where
        T: 'a + Debug,
    {
        Box::new(value)
    }

    fn allocated_bytes(&self) -> usize {
        0
    }
}
