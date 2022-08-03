use std::{
    cmp::Ordering,
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use fxhash::FxHashMap;

use crate::{
    change::Lamport,
    id::{Counter, ID},
    span::IdSpan,
    ClientID,
};

#[repr(transparent)]
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct VersionVector(FxHashMap<ClientID, Counter>);

impl Deref for VersionVector {
    type Target = FxHashMap<ClientID, Counter>;

    fn deref(&self) -> &Self::Target {
        &self.0
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
        Self(FxHashMap::default())
    }

    #[inline]
    pub fn set_end(&mut self, id: ID) {
        self.0.insert(id.client_id, id.counter + 1);
    }

    /// update the end counter of the given client, if the end is greater
    /// return whether updated
    #[inline]
    pub fn try_update_end(&mut self, id: ID) -> bool {
        if let Some(end) = self.0.get_mut(&id.client_id) {
            if *end < id.counter {
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
}

impl Default for VersionVector {
    fn default() -> Self {
        Self::new()
    }
}

impl From<FxHashMap<ClientID, Counter>> for VersionVector {
    fn from(map: FxHashMap<ClientID, Counter>) -> Self {
        Self(map)
    }
}

impl From<Vec<ID>> for VersionVector {
    fn from(vec: Vec<ID>) -> Self {
        let mut vv = VersionVector::new();
        for id in vec {
            vv.set_end(id);
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
}
