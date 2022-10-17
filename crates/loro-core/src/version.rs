use std::{
    cmp::Ordering,
    ops::{Deref, DerefMut, Sub},
};

use fxhash::FxHashMap;
use im::hashmap::HashMap as ImHashMap;

use crate::{
    change::Lamport,
    id::{Counter, ID},
    span::IdSpan,
    ClientID,
};

/// [VersionVector](https://en.wikipedia.org/wiki/Version_vector)
///
/// It's a map from [ClientID] to [Counter]. Its a right-open interval.
/// i.e. a [VersionVector] of `{A: 1, B: 2}` means that A has 1 atomic op and B has 2 atomic ops,
/// thus ID of `{client: A, counter: 1}` is out of the range.
///
/// In implementation, it's a immutable hash map with O(1) clone. Because
/// - we want a cheap clone op on vv;
/// - neighbor op's VersionVectors are very similar, most of the memory can be shared in
/// immutable hashmap
///
/// see also [im].
#[repr(transparent)]
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct VersionVector(ImHashMap<ClientID, Counter>);

impl Deref for VersionVector {
    type Target = ImHashMap<ClientID, Counter>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Sub for VersionVector {
    type Output = Vec<IdSpan>;

    fn sub(self, rhs: Self) -> Self::Output {
        let mut ans = Vec::new();
        for (client_id, &counter) in self.iter() {
            if let Some(&rhs_counter) = rhs.get(client_id) {
                if counter > rhs_counter {
                    ans.push(IdSpan::new(*client_id, rhs_counter, counter));
                }
            } else {
                ans.push(IdSpan::new(*client_id, 0, counter));
            }
        }

        ans
    }
}

impl PartialOrd for VersionVector {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        let mut self_greater = true;
        let mut other_greater = true;
        let mut eq = true;
        for (client_id, other_end) in other.iter() {
            if let Some(self_end) = self.get(client_id) {
                if self_end < other_end {
                    self_greater = false;
                    eq = false;
                }
                if self_end > other_end {
                    other_greater = false;
                    eq = false;
                }
            } else {
                self_greater = false;
                eq = false;
            }
        }

        for (client_id, _) in self.iter() {
            if other.contains_key(client_id) {
                continue;
            } else {
                other_greater = false;
                eq = false;
            }
        }

        if eq {
            Some(Ordering::Equal)
        } else if self_greater {
            Some(Ordering::Greater)
        } else if other_greater {
            Some(Ordering::Less)
        } else {
            None
        }
    }
}

impl DerefMut for VersionVector {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl VersionVector {
    #[inline]
    pub fn new() -> Self {
        Self(ImHashMap::new())
    }

    /// set the inclusive ending point. target id will be included by self
    #[inline]
    pub fn set_max(&mut self, id: ID) {
        self.0.insert(id.client_id, id.counter + 1);
    }

    /// set the exclusive ending point. target id will NOT be included by self
    #[inline]
    pub fn set_end(&mut self, id: ID) {
        self.0.insert(id.client_id, id.counter);
    }

    /// update the end counter of the given client, if the end is greater
    /// return whether updated
    #[inline]
    pub fn try_update_last(&mut self, id: ID) -> bool {
        if let Some(end) = self.0.get_mut(&id.client_id) {
            if *end < id.counter + 1 {
                *end = id.counter + 1;
                true
            } else {
                false
            }
        } else {
            self.0.insert(id.client_id, id.counter + 1);
            true
        }
    }

    pub fn get_missing_span(&self, target: &Self) -> Vec<IdSpan> {
        let mut ans = vec![];
        for (client_id, other_end) in target.iter() {
            if let Some(my_end) = self.get(client_id) {
                if my_end < other_end {
                    ans.push(IdSpan::new(*client_id, *my_end, *other_end));
                }
            } else {
                ans.push(IdSpan::new(*client_id, 0, *other_end));
            }
        }

        ans
    }

    pub fn merge(&mut self, other: &Self) {
        for (&client_id, &other_end) in other.iter() {
            if let Some(my_end) = self.get_mut(&client_id) {
                if *my_end < other_end {
                    *my_end = other_end;
                }
            } else {
                self.0.insert(client_id, other_end);
            }
        }
    }

    pub fn includes_id(&self, id: ID) -> bool {
        if let Some(end) = self.get(&id.client_id) {
            if *end > id.counter {
                return true;
            }
        }
        false
    }
}

impl Default for VersionVector {
    fn default() -> Self {
        Self::new()
    }
}

impl From<FxHashMap<ClientID, Counter>> for VersionVector {
    fn from(map: FxHashMap<ClientID, Counter>) -> Self {
        let mut im_map = ImHashMap::new();
        for (client_id, counter) in map {
            im_map.insert(client_id, counter);
        }
        Self(im_map)
    }
}

impl From<Vec<ID>> for VersionVector {
    fn from(vec: Vec<ID>) -> Self {
        let mut vv = VersionVector::new();
        for id in vec {
            vv.set_max(id);
        }

        vv
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord)]
pub(crate) struct TotalOrderStamp {
    pub(crate) lamport: Lamport,
    pub(crate) client_id: ClientID,
}

#[cfg(test)]
mod tests {
    use super::*;
    mod cmp {
        use super::*;
        #[test]
        fn test() {
            let a: VersionVector = vec![ID::new(1, 1), ID::new(2, 2)].into();
            let b: VersionVector = vec![ID::new(1, 1), ID::new(2, 2)].into();
            assert_eq!(a.partial_cmp(&b), Some(Ordering::Equal));

            let a: VersionVector = vec![ID::new(1, 2), ID::new(2, 1)].into();
            let b: VersionVector = vec![ID::new(1, 1), ID::new(2, 2)].into();
            assert_eq!(a.partial_cmp(&b), None);

            let a: VersionVector = vec![ID::new(1, 2), ID::new(2, 3)].into();
            let b: VersionVector = vec![ID::new(1, 1), ID::new(2, 2)].into();
            assert_eq!(a.partial_cmp(&b), Some(Ordering::Greater));

            let a: VersionVector = vec![ID::new(1, 0), ID::new(2, 2)].into();
            let b: VersionVector = vec![ID::new(1, 1), ID::new(2, 2)].into();
            assert_eq!(a.partial_cmp(&b), Some(Ordering::Less));
        }
    }

    #[test]
    fn im() {
        let mut a = VersionVector::new();
        a.set_max(ID::new(1, 1));
        a.set_max(ID::new(2, 1));
        let mut b = a.clone();
        b.merge(&vec![ID::new(1, 2), ID::new(2, 2)].into());
        assert!(a != b);
        assert_eq!(a.get(&1), Some(&2));
        assert_eq!(a.get(&2), Some(&2));
        assert_eq!(b.get(&1), Some(&3));
        assert_eq!(b.get(&2), Some(&3));
    }
}
