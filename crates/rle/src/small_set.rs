use std::{hash::Hash, mem::MaybeUninit};

use fxhash::FxHashSet;

pub enum SmallSet<T, const SIZE: usize> {
    Arr([Option<T>; SIZE]),
    Set(FxHashSet<T>),
}

impl<T: Eq + Hash, const SIZE: usize> SmallSet<T, SIZE> {
    pub fn new() -> Self {
        let a: MaybeUninit<[Option<T>; SIZE]> = MaybeUninit::zeroed();
        // SAFETY: we will init the array below
        let mut a = unsafe { a.assume_init_read() };
        for i in a.iter_mut() {
            *i = None;
        }
        SmallSet::Arr(a)
    }

    pub fn is_empty(&self) -> bool {
        match self {
            SmallSet::Arr(a) => a.is_empty(),
            SmallSet::Set(s) => s.is_empty(),
        }
    }

    /// Adds a value to the set.
    ///
    /// Returns whether the value was newly inserted. That is:
    ///
    /// - If the set did not previously contain this value, `true` is returned.
    /// - If the set already contained this value, `false` is returned.
    ///
    pub fn insert(&mut self, v: T) -> bool {
        match self {
            SmallSet::Arr(a) => {
                for i in a.iter_mut() {
                    if let Some(i) = i {
                        if *i == v {
                            return false;
                        }
                    } else {
                        *i = Some(v);
                        return true;
                    }
                }

                let mut set = FxHashSet::with_capacity_and_hasher(SIZE + 1, Default::default());
                for i in a.iter_mut() {
                    set.insert(std::mem::take(i).unwrap());
                }

                let ans = set.insert(v);
                *self = SmallSet::Set(set);
                ans
            }
            SmallSet::Set(set) => set.insert(v),
        }
    }

    pub fn contains(&mut self, v: &T) -> bool {
        match self {
            SmallSet::Arr(a) => {
                for i in a.iter_mut() {
                    if let Some(i) = i {
                        if i == v {
                            return true;
                        }
                    } else {
                        return false;
                    }
                }

                false
            }
            SmallSet::Set(set) => set.contains(v),
        }
    }

    pub fn remove(&mut self, v: &T) -> bool {
        match self {
            SmallSet::Arr(a) => {
                for i in a.iter_mut() {
                    if i.as_ref() == Some(v) {
                        *i = None;
                        return true;
                    }
                }

                false
            }
            SmallSet::Set(set) => set.remove(v),
        }
    }
}

impl<T: Eq + Hash, const SIZE: usize> Default for SmallSet<T, SIZE> {
    fn default() -> Self {
        Self::new()
    }
}
