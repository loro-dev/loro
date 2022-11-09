use std::{collections::hash_set::IntoIter, hash::Hash, mem::MaybeUninit};

use fxhash::FxHashSet;

pub enum SmallSet<T, const SIZE: usize> {
    Arr([Option<T>; SIZE], usize),
    Set(FxHashSet<T>),
}

pub enum SmallSetIter<T, const SIZE: usize> {
    Arr([Option<T>; SIZE], usize),
    Set(IntoIter<T>),
}

impl<T: Eq + Hash, const SIZE: usize> SmallSet<T, SIZE> {
    pub fn new() -> Self {
        let a: MaybeUninit<[Option<T>; SIZE]> = MaybeUninit::zeroed();
        // SAFETY: we will init the array below
        let mut a = unsafe { a.assume_init_read() };
        for i in a.iter_mut() {
            *i = None;
        }
        SmallSet::Arr(a, 0)
    }

    pub fn is_empty(&self) -> bool {
        match self {
            SmallSet::Arr(_, size) => *size == 0,
            SmallSet::Set(s) => s.is_empty(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            SmallSet::Arr(_, size) => *size,
            SmallSet::Set(s) => s.len(),
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
            SmallSet::Arr(a, i) => {
                *i += 1;
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
            SmallSet::Arr(a, _) => {
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
            SmallSet::Arr(a, i) => {
                *i -= 1;
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

impl<T, const SIZE: usize> Iterator for SmallSetIter<T, SIZE> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            SmallSetIter::Arr(arr, i) => {
                #[allow(clippy::needless_range_loop)]
                for index in *i..arr.len() {
                    if let Some(v) = std::mem::take(&mut arr[index]) {
                        *i += 1;
                        return Some(v);
                    }
                }
                None
            }
            SmallSetIter::Set(set) => set.next(),
        }
    }
}

impl<T: Eq + Hash, const SIZE: usize> IntoIterator for SmallSet<T, SIZE> {
    type Item = T;

    type IntoIter = SmallSetIter<T, SIZE>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            SmallSet::Arr(arr, _) => SmallSetIter::Arr(arr, 0),
            SmallSet::Set(arr) => SmallSetIter::Set(arr.into_iter()),
        }
    }
}
