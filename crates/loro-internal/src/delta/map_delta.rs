use std::hash::Hash;

use fxhash::FxHashMap;
use serde::{ser::SerializeStruct, Serialize};

use crate::{
    change::Lamport,
    handler::ValueOrContainer,
    id::{Counter, PeerID, ID},
    span::{HasId, HasLamport},
    InternalString, LoroValue,
};

#[derive(Default, Debug, Clone, Serialize)]
pub struct MapDelta {
    pub updated: FxHashMap<InternalString, MapValue>,
}

impl MapDelta {
    pub(crate) fn compose(&self, x: MapDelta) -> MapDelta {
        let mut updated = self.updated.clone();
        for (k, v) in x.updated.into_iter() {
            if let Some(old) = updated.get_mut(&k) {
                if v.lamport > old.lamport {
                    *old = v;
                }
            } else {
                updated.insert(k, v);
            }
        }
        MapDelta { updated }
    }

    #[inline]
    pub fn new() -> Self {
        MapDelta {
            updated: FxHashMap::default(),
        }
    }

    #[inline]
    pub fn with_entry(mut self, key: InternalString, map_value: MapValue) -> Self {
        self.updated.insert(key, map_value);
        self
    }
}

#[derive(Debug, Clone)]
pub struct MapValue {
    pub counter: Counter,
    pub value: Option<LoroValue>,
    pub lamport: (Lamport, PeerID),
}

#[derive(Default, Debug, Clone)]
pub struct ResolvedMapDelta {
    pub updated: FxHashMap<InternalString, ResolvedMapValue>,
}

#[derive(Debug, Clone)]
pub struct ResolvedMapValue {
    pub counter: Counter,
    pub value: Option<ValueOrContainer>,
    pub lamport: (Lamport, PeerID),
}

impl Hash for MapValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // value is not being hashed
        self.counter.hash(state);
        self.lamport.hash(state);
    }
}

impl HasId for MapValue {
    fn id_start(&self) -> crate::id::ID {
        ID::new(self.lamport.1, self.counter)
    }
}

impl HasLamport for MapValue {
    fn lamport(&self) -> Lamport {
        self.lamport.0
    }
}

impl PartialEq for MapValue {
    fn eq(&self, other: &Self) -> bool {
        self.lamport == other.lamport
    }
}

impl Eq for MapValue {}

impl PartialOrd for MapValue {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MapValue {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.lamport.cmp(&other.lamport)
    }
}

impl Serialize for MapValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = serializer.serialize_struct("MapValue", 2)?;
        s.serialize_field("value", &self.value)?;
        s.serialize_field("lamport", &self.lamport)?;
        s.end()
    }
}
