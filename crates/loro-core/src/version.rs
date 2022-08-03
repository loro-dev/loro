use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use fxhash::FxHashMap;

use crate::{
    change::Lamport,
    id::{Counter, ID},
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
