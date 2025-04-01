use fxhash::{FxHashMap, FxHashSet};
use serde::Serialize;
use std::fmt::Debug;

use crate::{InternalString, LoroValue};

#[derive(Clone, Debug, Serialize)]
pub struct ValuePair<T> {
    pub old: T,
    pub new: T,
}

impl From<(LoroValue, LoroValue)> for ValuePair<LoroValue> {
    fn from((old, new): (LoroValue, LoroValue)) -> Self {
        Self { old, new }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MapDiff<T> {
    pub added: FxHashMap<InternalString, T>,
    pub updated: FxHashMap<InternalString, ValuePair<T>>,
    pub deleted: FxHashMap<InternalString, T>,
}

impl<T> MapDiff<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(mut self, key: InternalString, value: T) -> Self {
        self.deleted.remove(&key);
        self.added.insert(key, value);
        self
    }

    pub fn delete(mut self, key: &InternalString, value: T) -> Self {
        self.added.remove(key);
        self.deleted.insert(key.clone(), value);
        self
    }

    pub fn compose(mut self, other: Self) -> Self {
        for (k, v) in other.added.into_iter() {
            if let Some(dv) = self.deleted.remove(&k) {
                self.updated.insert(k, ValuePair { old: dv, new: v });
            } else if let Some(ValuePair { old: _, new }) = self.updated.get_mut(&k) {
                // should not happen in transaction
                *new = v;
            } else {
                self.added.insert(k, v);
            }
        }

        for (k, v) in other.deleted.into_iter() {
            if let Some(av) = self.added.remove(&k) {
                self.deleted.insert(k, av);
            } else if let Some(ValuePair { old, .. }) = self.updated.remove(&k) {
                self.deleted.insert(k, old);
            } else {
                // delete old value
                self.deleted.insert(k, v);
            }
        }

        for (k, ValuePair { old, new }) in other.updated.into_iter() {
            if let Some(av) = self.added.get_mut(&k) {
                *av = new;
            } else if let Some(dv) = self.deleted.remove(&k) {
                self.updated.insert(k, ValuePair { old: dv, new });
            } else if let Some(ValuePair { old: _, new: n }) = self.updated.get_mut(&k) {
                *n = new
            } else {
                self.updated.insert(k, ValuePair { old, new });
            }
        }
        self
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MapDiffRaw<T> {
    pub(crate) added: FxHashMap<InternalString, T>,
    pub(crate) deleted: FxHashSet<InternalString>,
}

impl<T> Default for MapDiff<T> {
    fn default() -> Self {
        Self {
            added: Default::default(),
            updated: Default::default(),
            deleted: Default::default(),
        }
    }
}
