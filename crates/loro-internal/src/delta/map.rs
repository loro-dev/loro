use fxhash::{FxHashMap, FxHashSet};
use serde::Serialize;
use std::fmt::Debug;

use crate::{InternalString, LoroValue};

#[derive(Clone, Debug, Serialize)]
pub struct ValuePair {
    pub old: Option<LoroValue>,
    pub new: LoroValue,
}

impl From<(LoroValue, LoroValue)> for ValuePair {
    fn from((old, new): (LoroValue, LoroValue)) -> Self {
        ValuePair {
            old: Some(old),
            new,
        }
    }
}

impl From<(Option<LoroValue>, LoroValue)> for ValuePair {
    fn from((old, new): (Option<LoroValue>, LoroValue)) -> Self {
        ValuePair { old, new }
    }
}

#[derive(Default, Clone, Debug, Serialize)]
pub struct MapDiff {
    pub added: FxHashMap<InternalString, LoroValue>,
    pub updated: FxHashMap<InternalString, ValuePair>,
    pub deleted: FxHashSet<InternalString>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MapDiffRaw<T> {
    pub(crate) added: FxHashMap<InternalString, T>,
    pub(crate) deleted: FxHashSet<InternalString>,
}

impl<T> Default for MapDiffRaw<T> {
    fn default() -> Self {
        Self {
            added: Default::default(),
            deleted: Default::default(),
        }
    }
}

impl<T> MapDiffRaw<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(mut self, key: InternalString, value: T) -> Self {
        self.deleted.remove(&key);
        self.added.insert(key, value);
        self
    }

    pub fn delete(mut self, key: &InternalString) -> Self {
        self.added.remove(key);
        self.deleted.insert(key.into());
        self
    }

    pub fn compose(mut self, other: Self) -> Self {
        for (k, v) in other.added.into_iter() {
            self = self.insert(k, v);
        }
        for k in other.deleted {
            self = self.delete(&k);
        }
        self
    }
}
