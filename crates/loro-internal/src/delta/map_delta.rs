use std::sync::Arc;

use fxhash::FxHashMap;
use serde::{ser::SerializeStruct, Serialize};

use crate::{change::Lamport, id::PeerID, InternalString, LoroValue};

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
}

#[derive(Debug, Clone)]
pub struct MapValue {
    pub value: Option<Arc<LoroValue>>,
    pub lamport: (Lamport, PeerID),
}

impl Serialize for MapValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = serializer.serialize_struct("MapValue", 2)?;
        s.serialize_field("value", &self.value.as_deref())?;
        s.serialize_field("lamport", &self.lamport)?;
        s.end()
    }
}
