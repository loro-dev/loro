use std::{borrow::Borrow, hash::Hash};

use rustc_hash::FxHashMap;
use smallvec::SmallVec;

#[derive(Debug)]
pub struct ValueRegister<T>(RegisterStorage<T>);

#[derive(Debug)]
enum RegisterStorage<T> {
    Inline(SmallVec<[T; 8]>),
    Indexed {
        indices: FxHashMap<T, usize>,
        values: Vec<T>,
    },
}

impl<T: std::hash::Hash + Clone + Eq> Default for ValueRegister<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: std::hash::Hash + Clone + Eq> ValueRegister<T> {
    pub fn new() -> Self {
        Self(RegisterStorage::Indexed {
            indices: FxHashMap::default(),
            values: Vec::new(),
        })
    }

    pub fn for_block(inline: bool) -> Self {
        if inline {
            Self::inline()
        } else {
            Self::new()
        }
    }

    fn inline() -> Self {
        Self(RegisterStorage::Inline(SmallVec::new()))
    }

    /// Return the index of the given value. If it does not exist,
    /// insert it and return the new index.
    pub fn register(&mut self, key: &T) -> usize {
        if let RegisterStorage::Inline(values) = &mut self.0 {
            if let Some(index) = values.iter().position(|value| value == key) {
                return index;
            }
            if values.len() < values.inline_size() {
                let index = values.len();
                values.push(key.clone());
                return index;
            }
            let mut indices = FxHashMap::default();
            indices.reserve(values.len() + 1);
            indices.extend(values.iter().cloned().enumerate().map(|(i, v)| (v, i)));
            self.0 = RegisterStorage::Indexed {
                indices,
                values: std::mem::take(values).into_vec(),
            };
        }
        let RegisterStorage::Indexed { indices, values } = &mut self.0 else {
            unreachable!();
        };
        if let Some(index) = indices.get(key) {
            return *index;
        }
        let index = values.len();
        values.push(key.clone());
        indices.insert(key.clone(), index);
        index
    }

    pub fn get<Q>(&self, key: &Q) -> Option<usize>
    where
        T: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        match &self.0 {
            RegisterStorage::Inline(values) => {
                values.iter().position(|value| value.borrow() == key)
            }
            RegisterStorage::Indexed { indices, .. } => indices.get(key).copied(),
        }
    }

    pub fn len(&self) -> usize {
        self.vec().len()
    }

    #[allow(unused)]
    pub fn is_empty(&self) -> bool {
        self.vec().is_empty()
    }

    pub fn unwrap_vec(self) -> Vec<T> {
        match self.0 {
            RegisterStorage::Inline(values) => values.into_vec(),
            RegisterStorage::Indexed { values, .. } => values,
        }
    }

    #[allow(unused)]
    pub fn get_value(&self, index: usize) -> Option<&T> {
        self.vec().get(index)
    }

    pub fn vec(&self) -> &[T] {
        match &self.0 {
            RegisterStorage::Inline(values) => values,
            RegisterStorage::Indexed { values, .. } => values,
        }
    }
}
