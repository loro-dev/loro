use std::{
    hash::Hash,
    sync::Weak,
};

use fxhash::FxHashMap;
use loro_common::IdLp;
use serde::{ser::SerializeStruct, Serialize};

use crate::{
    change::Lamport, handler::ValueOrHandler, id::PeerID, span::HasLamport, InternalString, LoroDocInner, LoroValue,
};

#[derive(Default, Debug, Clone, Serialize)]
pub struct MapDelta {
    /// If the value is none, it's a uncreate op that should remove the entry from the
    /// map container.
    pub updated: FxHashMap<InternalString, Option<MapValue>>,
}

#[derive(Debug, Clone)]
pub struct MapValue {
    pub value: Option<LoroValue>,
    pub lamp: Lamport,
    pub peer: PeerID,
}

impl Ord for MapValue {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.lamp
            .cmp(&other.lamp)
            .then_with(|| self.peer.cmp(&other.peer))
    }
}

impl PartialOrd for MapValue {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for MapValue {
    fn eq(&self, other: &Self) -> bool {
        self.lamp == other.lamp && self.peer == other.peer
    }
}

impl Eq for MapValue {}

impl MapValue {
    pub fn idlp(&self) -> IdLp {
        IdLp::new(self.peer, self.lamp)
    }
}

#[derive(Default, Debug, Clone)]
pub struct ResolvedMapDelta {
    pub updated: FxHashMap<InternalString, ResolvedMapValue>,
}

#[derive(Debug, Clone)]
pub struct ResolvedMapValue {
    pub value: Option<ValueOrHandler>,
    pub idlp: IdLp,
}

impl ResolvedMapValue {
    pub(crate) fn from_map_value(v: MapValue, doc: &Weak<LoroDocInner>) -> Self {
        let doc = &doc.upgrade().unwrap();
        Self {
            idlp: IdLp::new(v.peer, v.lamp),
            value: v.value.map(|v| ValueOrHandler::from_value(v, doc)),
        }
    }

    /// This is used to indicate that the entry is unset. (caused by checkout to before the entry is created)
    pub fn new_unset() -> Self {
        Self {
            idlp: IdLp::new(PeerID::default(), Lamport::MAX),
            value: None,
        }
    }
}

impl MapDelta {
    pub(crate) fn compose(mut self, x: Self) -> Self {
        for (k, v) in x.updated.into_iter() {
            if let Some(old) = self.updated.get_mut(&k) {
                if &v > old {
                    *old = v;
                }
            } else {
                self.updated.insert(k, v);
            }
        }
        self
    }

    #[inline]
    pub fn new() -> Self {
        Self {
            updated: FxHashMap::default(),
        }
    }

    #[inline]
    pub fn with_entry(mut self, key: InternalString, map_value: MapValue) -> Self {
        self.updated.insert(key, Some(map_value));
        self
    }
}

impl ResolvedMapDelta {
    pub(crate) fn compose(&self, x: Self) -> Self {
        let mut updated = self.updated.clone();
        for (k, v) in x.updated.into_iter() {
            if let Some(old) = updated.get_mut(&k) {
                if v.idlp > old.idlp {
                    *old = v;
                }
            } else {
                updated.insert(k, v);
            }
        }
        Self { updated }
    }

    #[inline]
    pub fn new() -> Self {
        Self {
            updated: FxHashMap::default(),
        }
    }

    #[inline]
    pub fn with_entry(mut self, key: InternalString, map_value: ResolvedMapValue) -> Self {
        self.updated.insert(key, map_value);
        self
    }

    pub(crate) fn transform(&mut self, b: &Self, left_prior: bool) {
        for (k, _) in b.updated.iter() {
            if !left_prior {
                self.updated.remove(k);
            }
        }
    }
}

impl Hash for MapValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // value is not being hashed
        self.peer.hash(state);
        self.lamp.hash(state);
    }
}

impl HasLamport for MapValue {
    fn lamport(&self) -> Lamport {
        self.lamp
    }
}

impl Serialize for MapValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = serializer.serialize_struct("MapValue", 2)?;
        s.serialize_field("value", &self.value)?;
        s.serialize_field("lamport", &self.lamp)?;
        s.serialize_field("id", &self.idlp())?;
        s.end()
    }
}
