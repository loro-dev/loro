use rustc_hash::{FxHashMap, FxHashSet};
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
        ValuePair { old, new }
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
#[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::{MapDiff, ValuePair};
    use crate::InternalString;

    fn key(value: &str) -> InternalString {
        value.into()
    }

    fn map_diff(
        added: &[(&str, i32)],
        updated: &[(&str, (i32, i32))],
        deleted: &[(&str, i32)],
    ) -> MapDiff<i32> {
        let mut ans = MapDiff::new();
        for (k, v) in added {
            ans.added.insert(key(k), *v);
        }
        for (k, (old, new)) in updated {
            ans.updated.insert(
                key(k),
                ValuePair {
                    old: *old,
                    new: *new,
                },
            );
        }
        for (k, v) in deleted {
            ans.deleted.insert(key(k), *v);
        }
        ans
    }

    fn assert_map_diff_eq(actual: MapDiff<i32>, expected: MapDiff<i32>) {
        assert_eq!(actual.added, expected.added);
        assert_eq!(actual.deleted, expected.deleted);

        assert_eq!(actual.updated.len(), expected.updated.len());
        for (key, expected_pair) in expected.updated {
            let actual_pair = actual.updated.get(&key).unwrap_or_else(|| {
                panic!("missing updated entry for key {:?}", key);
            });
            assert_eq!(actual_pair.old, expected_pair.old);
            assert_eq!(actual_pair.new, expected_pair.new);
        }
    }

    #[test]
    fn insert_replaces_previous_delete_marker() {
        let diff = map_diff(&[], &[], &[("key", 1)]).insert(key("key"), 2);
        assert_map_diff_eq(diff, map_diff(&[("key", 2)], &[], &[]));
    }

    #[test]
    fn delete_removes_pending_add_and_records_deleted_value() {
        let diff = map_diff(&[("key", 1)], &[], &[]).delete(&key("key"), 1);
        assert_map_diff_eq(diff, map_diff(&[], &[], &[("key", 1)]));
    }

    #[test]
    fn compose_add_then_update_keeps_latest_value_in_added() {
        let lhs = map_diff(&[("key", 1)], &[], &[]);
        let rhs = map_diff(&[], &[("key", (1, 2))], &[]);

        assert_map_diff_eq(lhs.compose(rhs), map_diff(&[("key", 2)], &[], &[]));
    }

    #[test]
    fn compose_delete_then_add_becomes_update_with_original_old_value() {
        let lhs = map_diff(&[], &[], &[("key", 1)]);
        let rhs = map_diff(&[("key", 2)], &[], &[]);

        assert_map_diff_eq(lhs.compose(rhs), map_diff(&[], &[("key", (1, 2))], &[]));
    }

    #[test]
    fn compose_update_then_delete_keeps_initial_old_value() {
        let lhs = map_diff(&[], &[("key", (1, 2))], &[]);
        let rhs = map_diff(&[], &[], &[("key", 2)]);

        assert_map_diff_eq(lhs.compose(rhs), map_diff(&[], &[], &[("key", 1)]));
    }

    #[test]
    fn compose_multiple_updates_preserves_first_old_and_latest_new() {
        let lhs = map_diff(&[], &[("key", (1, 2))], &[]);
        let rhs = map_diff(&[], &[("key", (2, 3))], &[]);
        let third = map_diff(&[], &[("key", (3, 4))], &[]);

        assert_map_diff_eq(
            lhs.compose(rhs).compose(third),
            map_diff(&[], &[("key", (1, 4))], &[]),
        );
    }

    #[test]
    fn compose_is_key_local_and_preserves_other_entries() {
        let lhs = map_diff(&[("added", 10)], &[("updated", (1, 2))], &[("deleted", 3)]);
        let rhs = map_diff(&[("added", 11)], &[("updated", (2, 3))], &[("deleted", 4)]);

        assert_map_diff_eq(
            lhs.compose(rhs),
            map_diff(&[("added", 11)], &[("updated", (1, 3))], &[("deleted", 4)]),
        );
    }
}
